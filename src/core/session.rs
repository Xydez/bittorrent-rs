use std::{
	collections::HashMap,
	sync::Arc
};

use tokio::sync::{
	Mutex,
	RwLock,
	Semaphore
};

use crate::core::{
	configuration::Configuration,
	event::{
		Event,
		TorrentEvent
	},
	peer::Peer,
	piece_download::PieceDownload,
	torrent::Torrent
};

/* Type definitions */

#[derive(Debug)]
enum Command {
	AddTorrent(tokio::sync::oneshot::Sender<TorrentPtr>, Torrent),
	GetTorrent(tokio::sync::oneshot::Sender<TorrentPtr>, TorrentId),
	GetConfig(tokio::sync::oneshot::Sender<Arc<Configuration>>),
	Shutdown
}

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReceiver = tokio::sync::mpsc::UnboundedReceiver<Event>;
type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
type CommandReceiver = tokio::sync::mpsc::UnboundedReceiver<Command>;
pub type PieceId = u32; // TODO: Newtype
pub type TorrentId = u32; // TODO: Newtype
pub type TorrentPtr = Arc<RwLock<Torrent>>;
pub type PeerPtr = Arc<Mutex<Peer>>;
pub type PieceDownloadPtr = Arc<Mutex<PieceDownload>>;

pub trait EventCallback = Fn(&Session, &Event);

pub struct SessionHandle {
	cmd_tx: CommandSender
}

impl SessionHandle {
	/// Adds a torrent to the session
	pub async fn add_torrent(&self, torrent: Torrent) -> TorrentPtr {
		let (tx, rx) = tokio::sync::oneshot::channel();

		self.cmd_tx.send(Command::AddTorrent(tx, torrent)).unwrap();

		rx.await.unwrap()
	}

	/// Get a [`TorrentPtr`] from a [`TorrentId`]
	///
	/// TODO: Should return an Option
	pub async fn torrent(&self, torrent_id: TorrentId) -> TorrentPtr {
		let (tx, rx) = tokio::sync::oneshot::channel();

		self.cmd_tx
			.send(Command::GetTorrent(tx, torrent_id))
			.unwrap();

		rx.await.unwrap()
	}

	/// Get the [`Configuration`] of the session
	pub async fn config(&self) -> Arc<Configuration> {
		let (tx, rx) = tokio::sync::oneshot::channel();

		self.cmd_tx.send(Command::GetConfig(tx)).unwrap();

		rx.await.unwrap()
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
	torrents: HashMap<u32, TorrentPtr>,
	config: Arc<Configuration>,
	//listeners: Vec<Box<dyn EventCallback + 'static>>,
	/// Transmits events to the event loop
	tx: EventSender,
	/// Receives events within the event loop
	rx: EventReceiver,
	/// Receives commands from the [`SessionHandle`]
	cmd_rx: CommandReceiver,
	/// Sends events to event listeners
	broadcast_tx: tokio::sync::broadcast::Sender<Event>,
	/// Semaphore to track the number of pieces that can be verified at the same time
	verification_semaphore: Arc<Semaphore>
}

impl Session {
	/// Constructs a session with the default configuration
	pub fn spawn() -> (Arc<SessionHandle>, tokio::sync::broadcast::Receiver<Event>) {
		Session::spawn_with_config(Configuration::default())
	}

	/// Constructs a session with the provided configuration
	pub fn spawn_with_config(
		config: Configuration
	) -> (Arc<SessionHandle>, tokio::sync::broadcast::Receiver<Event>) {
		let config = Arc::new(config);
		let verification_semaphore = Arc::new(Semaphore::new(config.verification_jobs));

		let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
		let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
		let (broadcast_tx, broadcast_rx) = tokio::sync::broadcast::channel(64);

		let session = Session {
			config,
			tx,
			rx,
			cmd_rx,
			broadcast_tx,
			verification_semaphore,
			torrents: HashMap::new()
		};

		tokio::spawn(session.run());

		let handle = Arc::new(SessionHandle { cmd_tx });

		(handle, broadcast_rx)
	}

	/// Starts the event loop of the session
	///
	/// The event loop keeps on running until [Event::Stopped] is received
	async fn run(mut self) {
		self.tx.send(Event::Started).unwrap();

		loop {
			tokio::select! {
				Some(event) = self.rx.recv() => {
					let should_close = matches!(event, Event::Stopped);

					self.handle(&event).await;
					self.broadcast_tx.send(event).unwrap();

					// Inform the listeners of the event
					/*
					for listener in &self.listeners {
						listener(&self, &event);
					}
					*/

					// Stop the event loop if the session has stopped
					if should_close {
						break;
					}
				},
				command = self.cmd_rx.recv() => {
					if let Some(command) = command {
						match command {
							Command::AddTorrent(sender, torrent) => sender.send(self.add_torrent(torrent)).unwrap(),
							Command::GetTorrent(sender, torrent_id) => sender.send(self.torrents[&torrent_id].clone()).unwrap(),
							Command::GetConfig(sender) => sender.send(self.config.clone()).unwrap(),
							Command::Shutdown => self.tx.send(Event::Stopped).unwrap()
						}
					} else {
						break;
					}
				}
			}
		}

		self.shutdown().await;
	}

	/// Adds a torrent to the session
	///
	/// The torrent will not start downloading until [start] is called
	fn add_torrent(&mut self, torrent: Torrent) -> TorrentPtr {
		let torrent_id = torrent.id();
		let torrent_ptr = Arc::new(RwLock::new(torrent));

		self.torrents.insert(torrent_id, torrent_ptr.clone());
		self.tx
			.send(Event::TorrentEvent(torrent_id, TorrentEvent::Added))
			.unwrap();

		torrent_ptr
	}

	fn torrent(&self, id: TorrentId) -> &TorrentPtr {
		&self.torrents[&id]
	}

	async fn shutdown(self) {
		for (_, torrent) in self.torrents {
			torrent.write().await.shutdown().await;
		}
	}
}

mod handler;
