use std::{collections::HashMap, sync::Arc};

use tokio::sync::{Mutex, Semaphore};

use self::task::{Command, CommandSender};
use super::torrent::TorrentHandle;
use crate::{
	core::{
		configuration::Configuration,
		event::{Event, TorrentEvent},
		piece_download::PieceDownload,
	},
	peer::Peer,
	torrent::{Torrent, TorrentId},
};

mod handler;
mod task;

/* Type definitions */

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReceiver = tokio::sync::mpsc::UnboundedReceiver<Event>;

pub type TorrentPtr = Arc<Torrent>;
pub type PeerPtr = Arc<Mutex<Peer>>;
pub type PieceDownloadPtr = Arc<Mutex<PieceDownload>>;

pub trait EventCallback = Fn(&Session, &Event);

pub struct SessionHandle {
	data: Arc<Mutex<Session>>,
	cmd_tx: CommandSender,
}

impl SessionHandle {
	/// Adds a torrent to the session
	pub async fn add_torrent(&self, torrent: Torrent) -> TorrentId {
		let (tx, rx) = tokio::sync::oneshot::channel();

		self.cmd_tx.send(Command::AddTorrent(tx, torrent)).unwrap();

		rx.await.unwrap()
	}

	/// Get a [`TorrentPtr`] from a [`TorrentId`]
	pub async fn torrent(&self, torrent_id: TorrentId) -> TorrentPtr {
		self.data.lock().await.torrent(torrent_id).torrent.clone()
	}

	/// Get the [`Configuration`] of the session
	pub async fn config(&self) -> Arc<Configuration> {
		self.data.lock().await.config.clone()
	}

	/// Sends a shutdown command to the session
	pub fn shutdown(&self) {
		self.cmd_tx.send(Command::Shutdown).unwrap();
	}

	/// Waits until the session has stopped
	pub async fn join(&self) {
		self.cmd_tx.closed().await;
	}
}

impl Drop for SessionHandle {
	fn drop(&mut self) {
		if !self.cmd_tx.is_closed() {
			self.cmd_tx.send(Command::Shutdown).unwrap();
		}
	}
}
pub struct Session {
	torrents: HashMap<TorrentId, Arc<TorrentHandle>>, // <- TODO: Maintain JoinSet as well, maybe it should be in the session task or here?
	config: Arc<Configuration>,
	/// Transmits events to the event loop
	tx: EventSender, // TODO: Still unsure if I prefer callbacks instead
	/// Sends events to event listeners
	broadcast_tx: tokio::sync::broadcast::Sender<Event>,
	/// Semaphore to track the number of pieces that can be verified at the same time
	verification_semaphore: Arc<Semaphore>,
}

impl Session {
	/// Constructs a session with the default configuration
	pub fn spawn() -> (SessionHandle, tokio::sync::broadcast::Receiver<Event>) {
		Session::spawn_with_config(Configuration::default())
	}

	/// Constructs a session with the provided configuration
	pub fn spawn_with_config(
		config: Configuration,
	) -> (SessionHandle, tokio::sync::broadcast::Receiver<Event>) {
		let config = Arc::new(config);
		let verification_semaphore = Arc::new(Semaphore::new(config.verification_jobs));

		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
		let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel(64);

		let data = Session {
			config,
			tx,
			broadcast_tx,
			verification_semaphore,
			torrents: HashMap::new(),
		};

		let data = Arc::new(Mutex::new(data));

		tokio::spawn(task::run(data.clone(), cmd_rx, rx));

		let handle = SessionHandle { data, cmd_tx };

		(handle, broadcast_rx)
	}

	/// Adds a torrent to the session
	async fn add_torrent(&mut self, torrent: Torrent) -> TorrentId {
		let (torrent_handle, fut) = torrent.spawn(self.tx.clone(), self.config.clone());

		// TODO: Add it to some sort of JoinMap instead
		tokio::spawn(fut);

		let torrent_id = torrent_handle.torrent.id;

		self.torrents.insert(torrent_id, Arc::new(torrent_handle));
		self.tx
			.send(Event::TorrentEvent(torrent_id, TorrentEvent::Added))
			.unwrap();

		torrent_id
	}

	pub fn torrent(&self, id: TorrentId) -> &TorrentHandle {
		&self.torrents[&id]
	}
}
