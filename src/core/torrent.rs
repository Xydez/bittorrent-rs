use std::{
	collections::{
		BTreeMap,
		HashMap
	},
	sync::{
		atomic::AtomicU32,
		Arc
	}
};

use tokio::sync::Mutex;

use crate::{
	core::{
		algorithm::Picker,
		configuration::Configuration,
		piece::{
			Piece,
			Priority,
			State
		},
		piece_download::PieceDownload,
		session::{
			PeerPtr,
			PieceId,
			TorrentId
		},
		util,
		worker
	},
	io::store::Store,
	protocol::{
		metainfo::MetaInfo,
		tracker::{
			Announce,
			Response,
			Tracker
		}
	}
};

#[derive(Debug)]
pub struct WorkerHandle {
	pub peer: PeerPtr,
	pub mode_tx: tokio::sync::watch::Sender<worker::Mode>,
	pub task: tokio::task::JoinHandle<worker::Result<()>>
}

/// Atomic counter used to generate torrent identifiers
static ID_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct Torrent {
	/// Identifier of the torrent
	id: TorrentId,
	/// Meta info of the torrent
	pub meta_info: MetaInfo,
	/// List of all workers
	pub peers: Vec<WorkerHandle>,
	/// Store of the torrent
	pub store: Arc<Mutex<Box<dyn Store>>>,
	/// List of all pieces
	pub pieces: Vec<Piece>,
	/// Piece picker
	pub picker: Picker,
	/// Ongoing piece downloads
	pub downloads: HashMap<u32, Arc<Mutex<PieceDownload>>>,
	// TODO: Implement multiple trackers
	pub tracker: Tracker
}

impl Torrent {
	pub fn new(meta_info: MetaInfo, store: impl Store + 'static) -> Torrent {
		let id = ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		let mut pieces = Vec::new();

		for _ in 0..meta_info.pieces.len() {
			pieces.push(Piece::default());
		}

		let tracker = Tracker::new(&meta_info.announce);

		Torrent {
			id,
			meta_info,
			peers: Vec::new(),
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

	pub(crate) async fn shutdown(&mut self) {
		for peer in self.peers.drain(..) {
			drop(peer.mode_tx);
			peer.task.await.unwrap().unwrap();
		}
	}
}

impl std::fmt::Display for Torrent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Torrent {}", self.id)
	}
}
