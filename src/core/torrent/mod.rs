use std::{
	collections::{BTreeMap, HashMap},
	future::Future,
	sync::{atomic::AtomicU32, Arc}
};

use thiserror::Error;
use tokio::sync::{Mutex, MutexGuard, OwnedSemaphorePermit};

use self::{
	resume::{Resume, ResumeData},
	task::{Command, CommandSender}
};
use super::{bitfield::Bitfield, peer::Peer, statistics::Statistics};
use crate::{
	core::{
		configuration::Configuration,
		piece::{self, Piece, PieceId, Priority, State},
		piece_download::PieceDownload,
		session::{EventSender, TorrentPtr},
		util
	},
	io::store::Store,
	protocol::{metainfo::MetaInfo, tracker::Tracker}
};

pub mod resume;
mod task;

#[derive(Error, Debug)]
pub enum ResumeError {
	#[error("An error occurred while reading or writing data")]
	IOError(#[from] std::io::Error),
	#[error("The checksum of the provided store does not match the deserialized store")]
	InvalidChecksum,
	#[error(transparent)]
	Other(#[from] Box<dyn std::error::Error>)
}

/// Atomic counter used to generate torrent identifiers
static TORRENT_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

pub type WorkerId = u32;

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
pub struct TorrentHandle {
	pub torrent: TorrentPtr,
	cmd_tx: CommandSender,
	conn_tx: tokio::sync::mpsc::UnboundedSender<(Peer, Option<OwnedSemaphorePermit>)>
}

impl TorrentHandle {
	pub async fn shutdown(&self) {
		self.cmd_tx.send(Command::Shutdown).unwrap();
	}

	pub(crate) async fn complete(&self) {
		self.cmd_tx.send(Command::Complete).unwrap();
	}

	pub async fn join(self) {
		self.cmd_tx.closed().await;
	}

	/// Adds a connection initiated by the client
	pub(crate) fn add_conn(&self, peer: Peer) {
		self.conn_tx.send((peer, None)).unwrap();
	}
}

#[derive(Debug)]
pub struct Torrent {
	/// Identifier of the torrent
	pub id: TorrentId,
	/// Meta info of the torrent
	pub meta_info: MetaInfo,
	/// Store of the torrent
	pub store: Mutex<Box<dyn Store>>,
	/// Mutable state of the torrent
	state: Mutex<TorrentState>
}

pub struct TorrentLock<'a> {
	pub(crate) torrent: &'a Torrent,
	state: MutexGuard<'a, TorrentState>
}

#[derive(Debug)]
pub struct TorrentState {
	/// List of all pieces
	pub pieces: Vec<Piece>,
	/// Map of piece id to download
	pub downloads: HashMap<PieceId, Arc<Mutex<PieceDownload>>>,
	// TODO: Implement multiple trackers
	pub tracker: Tracker,
	/// Statistics for the torrent
	pub stats: Statistics
}

impl Torrent {
	pub fn new(meta_info: MetaInfo, store: impl Store + 'static) -> Self {
		let pieces = vec![Piece::default(); meta_info.pieces.len()];
		let store = Box::new(store);

		Self::new_impl(meta_info, store, pieces)
	}

	pub fn new_resumed<S: Store + 'static, T: Resume<S>>(
		resume: T,
		resume_data: ResumeData
	) -> Result<Torrent, T::Error> {
		let meta_info = resume_data.meta_info.clone();
		let store = Box::new(resume.resume(&resume_data)?);
		let pieces = resume_data
			.pieces
			.iter()
			.map(|piece| Piece {
				state: if *piece { State::Done } else { State::Pending },
				..Default::default()
			})
			.collect::<Vec<_>>();

		Ok(Self::new_impl(meta_info, store, pieces))
	}

	pub fn new_impl(meta_info: MetaInfo, store: Box<dyn Store>, pieces: Vec<Piece>) -> Self {
		let id = TorrentId::gen();
		let tracker = Tracker::new(&meta_info.announce);
		let store = Mutex::new(store);

		let state = Mutex::new(TorrentState {
			pieces,
			downloads: HashMap::new(),
			tracker,
			stats: Statistics::default()
		});

		Torrent {
			id,
			meta_info,
			store,
			state
		}
	}

	pub fn spawn(
		self,
		event_tx: EventSender,
		config: Arc<Configuration>
	) -> (TorrentHandle, impl Future<Output = task::Result<()>>) {
		let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
		let (conn_tx, conn_rx) = tokio::sync::mpsc::unbounded_channel();
		let torrent = Arc::new(self);

		let handle = TorrentHandle {
			torrent: torrent.clone(),
			cmd_tx,
			conn_tx: conn_tx.clone()
		};

		let fut = task::run(torrent, cmd_rx, event_tx, (conn_tx, conn_rx), config);

		(handle, fut)
	}

	pub async fn lock(&self) -> TorrentLock<'_> {
		TorrentLock {
			torrent: self,
			state: self.state.lock().await
		}
	}

	pub async fn resume_data(&self) -> ResumeData {
		let pieces = self
			.state
			.lock()
			.await
			.pieces
			.iter()
			.map(|piece| piece.state == piece::State::Done)
			.collect::<Vec<_>>();

		ResumeData {
			meta_info: self.meta_info.clone(),
			pieces,
			checksum: self.store.lock().await.checksum().unwrap()
		}
	}
}

impl<'a> TorrentLock<'a> {
	pub fn state(&self) -> &TorrentState {
		&self.state
	}

	pub(crate) fn state_mut(&mut self) -> &mut TorrentState {
		&mut self.state
	}

	/// Returns an iterator of the torrent's [`Piece`]s and their respective
	/// [`PieceId`]s
	pub fn pieces(&self) -> impl Iterator<Item = (PieceId, &Piece)> {
		self.state
			.pieces
			.iter()
			.enumerate()
			.map(|(i, piece)| (i as u32, piece))
	}

	/// Calculate the number of bytes left until the download is complete. Only
	/// accounts for verified pieces.
	pub fn left(&self) -> usize {
		self.torrent.meta_info.size
			- self
				.pieces()
				.filter(|(_, piece)| piece.state >= State::Writing)
				.map(|(piece_id, _)| util::piece_size(piece_id, &self.torrent.meta_info))
				.sum::<usize>()
	}

	/// Returns the pieces of the torrent grouped by [`Priority`]
	pub fn pieces_grouped(&self) -> BTreeMap<Priority, Vec<(usize, &Piece)>> {
		util::group_by_key(self.state.pieces.iter().enumerate(), |(_, piece)| {
			piece.priority
		})
	}

	/// Returns `true` if all pieces in the torrent are [`State::Done`]
	pub fn is_done(&self) -> bool {
		self.state
			.pieces
			.iter()
			.all(|piece| piece.state == State::Done)
	}

	/// Creates a bitfoield from the torrent's current pieces
	pub fn bitfield(&self) -> Bitfield {
		Bitfield::from_bytes_length(
			&self
				.state
				.pieces
				.chunks(8)
				.map(|pieces| {
					pieces.iter().enumerate().fold(0u8, |acc, (i, piece)| {
						if piece.state == piece::State::Done {
							acc + (1 << (7 - i))
						} else {
							acc
						}
					})
				})
				.collect::<Vec<u8>>(),
			self.state.pieces.len()
		)
		.unwrap()
	}
}

impl std::fmt::Display for Torrent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Torrent {}", self.id)
	}
}
