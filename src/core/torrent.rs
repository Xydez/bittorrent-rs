use std::{
	collections::{BTreeMap, HashMap},
	sync::{atomic::AtomicU32, Arc}
};

use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::task::JoinMap;

use crate::{
	core::{
		algorithm::Picker,
		configuration::Configuration,
		piece::{self, Piece, PieceId, Priority, State},
		piece_download::PieceDownload,
		session::{EventSender, PeerPtr, TorrentPtr},
		util, worker
	},
	io::store::Store,
	protocol::{
		metainfo::MetaInfo,
		tracker::{Announce, Response, Tracker}
	}
};

#[derive(Error, Debug)]
pub enum ResumeError {
	#[error("An error occurred while reading or writing data")]
	IOError(#[from] std::io::Error),
	#[error("An error occurred while serializing or deserializing data")]
	BincodeError(#[from] bincode::Error),
	#[error("The checksum of the provided store does not match the deserialized store")]
	InvalidChecksum,
	#[error(transparent)]
	Other(#[from] Box<dyn std::error::Error>)
}

#[derive(Debug)]
pub struct WorkerHandle {
	pub peer: PeerPtr,
	pub mode_tx: tokio::sync::watch::Sender<worker::Mode>
}

static WORKER_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct ResumeData<'a> {
	pub(crate) meta_info: &'a MetaInfo,
	pub(crate) pieces: &'a Vec<bool>,
	pub(crate) checksum: u64
}

/// Atomic counter used to generate torrent identifiers
static TORRENT_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct TorrentId(pub(crate) u32);

impl TorrentId {
	pub(crate) fn gen() -> Self {
		TorrentId(TORRENT_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
	}
}

impl std::fmt::Display for TorrentId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

#[derive(Debug)]
pub struct Torrent {
	/// Identifier of the torrent
	id: TorrentId,
	/// Meta info of the torrent
	pub meta_info: MetaInfo,
	/// Map of a worker's id to handle
	pub peers: HashMap<u32, WorkerHandle>,
	/// JoinMap of a worker's id to the result of the process
	workers: JoinMap<u32, worker::Result<()>>,
	/// Store of the torrent
	pub store: Arc<Mutex<Box<dyn Store>>>,
	/// List of all pieces
	pub pieces: Vec<Piece>,
	/// Piece picker
	pub picker: Picker,
	/// Map of piece id to download
	pub downloads: HashMap<PieceId, Arc<Mutex<PieceDownload>>>,
	// TODO: Implement multiple trackers
	pub tracker: Tracker
}

impl Torrent {
	pub fn new(meta_info: MetaInfo, store: impl Store + 'static) -> Torrent {
		let id = TorrentId::gen();
		let mut pieces = Vec::new();

		for _ in 0..meta_info.pieces.len() {
			pieces.push(Piece::default());
		}

		let tracker = Tracker::new(&meta_info.announce);

		Torrent {
			id,
			meta_info,
			peers: HashMap::new(),
			workers: JoinMap::new(),
			pieces,
			picker: Picker { end_game: false },
			store: Arc::new(Mutex::new(Box::new(store))),
			downloads: HashMap::new(),
			tracker
		}
	}

	/// Returns the identifier of the torrent
	pub fn id(&self) -> TorrentId {
		self.id
	}

	/// Returns an iterator of all pieces with state [`State::Pending`]
	pub fn pending_pieces(&self) -> impl Iterator<Item = (PieceId, &Piece)> {
		self.pieces
			.iter()
			.enumerate()
			.filter(|(_, piece)| piece.state == State::Pending)
			.map(|(i, p)| (i as PieceId, p))
	}

	/// Returns an iterator of all pieces with state [`State::Downloading`]
	pub fn downloading_pieces(&self) -> impl Iterator<Item = (PieceId, &Piece)> {
		self.pieces
			.iter()
			.enumerate()
			.filter(|(_, piece)| piece.state == State::Downloading)
			.map(|(i, p)| (i as PieceId, p))
	}

	/// Returns an iterator of all pieces with state [`State::Pending`] or [`State::Downloading`]
	pub fn active_pieces(&self) -> impl Iterator<Item = (PieceId, &Piece)> {
		self.pending_pieces().chain(self.downloading_pieces())
	}

	/// Calculate the number of completed pieces (`piece.state` is greater than or equal to [`State::Verified`])
	pub fn complete_pieces(&self) -> impl Iterator<Item = (PieceId, &Piece)> {
		self.pieces
			.iter()
			.enumerate()
			.filter(|(_, piece)| piece.state >= State::Verified)
			.map(|(i, p)| (i as PieceId, p))
	}

	/// Returns the pieces of the torrent grouped by [`Priority`]
	pub fn pieces_grouped(&self) -> BTreeMap<Priority, Vec<(usize, &Piece)>> {
		util::group_by_key(self.pieces.iter().enumerate(), |(_, piece)| piece.priority)
	}

	/// Returns `true` if all pieces in the torrent are [`State::Done`]
	pub fn is_done(&self) -> bool {
		self.pieces.iter().all(|piece| piece.state == State::Done)
	}

	/// Announces a torrent
	pub(crate) async fn try_announce(&mut self, config: &Configuration) -> Option<Response> {
		let mut i = 0;

		let announce = Announce {
			info_hash: self.meta_info.info_hash,
			peer_id: config.peer_id,
			ip: None,
			port: 8000,
			uploaded: 0,
			downloaded: 0,
			left: 0,
			event: None // TODO: Should probably be started on the first announce
		};

		loop {
			if i < config.announce_retries {
				log::trace!("Announcing to tracker \"{}\"", self.tracker.announce_url());

				let response = self.tracker.announce(&announce).await;

				match response {
					Ok(response) => break Some(response),
					Err(error) => log::error!("Failed to announce: {}", error)
				}
			} else {
				break None;
			}

			i += 1;
		}
	}

	pub(crate) fn spawn_worker(
		&mut self,
		config: Arc<Configuration>,
		torrent: TorrentPtr,
		peer: PeerPtr,
		event_tx: EventSender
	) -> u32 {
		let id = WORKER_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

		let (mode_tx, mode_rx) = tokio::sync::watch::channel(worker::Mode {
			download: true,
			seed: false
		});

		self.workers.spawn(
			id,
			worker::run(config, torrent, peer.clone(), event_tx, mode_rx)
		);

		self.peers.insert(id, WorkerHandle { mode_tx, peer });

		id
	}

	pub(crate) async fn shutdown(&mut self) {
		log::debug!("Shutting down peers");

		self.peers.clear();

		while let Some((id, result)) = self.workers.join_next().await {
			if let Err(error) = result.unwrap() {
				log::error!("[Worker {id}] An error occurred while shutting down: {error}");
			}
		}
	}
}

#[cfg(feature = "resume")]
impl Torrent {
	pub fn serialize_into(&self, mut writer: impl std::io::Write) -> Result<(), ResumeError> {
		let checksum = self.store.blocking_lock().checksum().map_err(|_| {
			bincode::Error::new(bincode::ErrorKind::Custom(
				"Failed to calculate checksum of store".to_string()
			))
		})?;

		let pieces = self
			.pieces
			.iter()
			.map(|piece| piece.state == piece::State::Done)
			.collect::<Vec<_>>();

		bincode::serialize_into(&mut writer, &self.meta_info)?;
		bincode::serialize_into(&mut writer, &pieces)?;
		bincode::serialize_into(&mut writer, &checksum)?;

		Ok(())
	}

	/// Deserialize a torrent from a reader
	///
	/// The callback `load_store` should load the store the provided [`MetaInfo`] corresponds to with the requested pieces
	pub fn deserialize_from<F, S>(
		mut reader: impl std::io::Read,
		load_store: F
	) -> Result<Self, ResumeError>
	where
		F: FnOnce(ResumeData) -> S,
		S: Store + 'static
	{
		let meta_info: MetaInfo = bincode::deserialize_from(&mut reader)?;
		let pieces: Vec<bool> = bincode::deserialize_from(&mut reader)?;
		let checksum: u64 = bincode::deserialize_from(&mut reader)?;

		let store = load_store(ResumeData {
			meta_info: &meta_info,
			pieces: &pieces,
			checksum
		});

		let mut torrent = Torrent::new(meta_info, store);
		torrent.pieces = pieces
			.iter()
			.map(|val| Piece {
				state: if *val {
					piece::State::Done
				} else {
					piece::State::Pending
				},
				..Piece::default()
			})
			.collect::<Vec<Piece>>();

		Ok(torrent)
	}
}

impl std::fmt::Display for Torrent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Torrent {}", self.id)
	}
}
