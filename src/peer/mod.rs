use std::{
	future::Future,
	sync::{atomic::AtomicU32, Arc}
};

use common::util;
use protocol::{
	extensions::Extensions,
	peer_id::PeerId,
	wire::{
		self,
		connection::{Handshake, Wire},
		message::Message
	}
};
use thiserror::Error;
use tokio::sync::Mutex;

use super::{
	session::{PeerPtr, TorrentPtr},
	torrent::WorkerId
};
use crate::core::{
	bitfield::Bitfield,
	configuration::Configuration,
	event::{PeerEvent, Sender}
};

mod block_worker;
pub(crate) mod task;

#[derive(Error, Debug)]
pub enum Error {
	#[error("Request timed out")]
	Timeout,
	#[error("An error occurred within the wire protocol.")]
	WireError(#[from] wire::connection::Error),
	#[error("The peer sent an illegal message: {0}")]
	IllegalMessage(Message)
}

pub type Result<T> = std::result::Result<T, Error>;

/// Atomic counter used to generate worker identifiers
static WORKER_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct PeerHandle {
	data: PeerPtr,
	pub(crate) _mode_tx: tokio::sync::watch::Sender<task::Mode> // TODO: Should be managed in torrent task
}

impl PeerHandle {
	pub fn data(&self) -> &PeerPtr {
		&self.data
	}
}

/// Abstraction of Wire that maintains peer state
#[derive(Debug)]
pub struct Peer {
	id: WorkerId,
	wire: Wire,
	peer_id: [u8; 20],

	/// The extensions the peer supports
	extensions: Extensions,

	/// Whether we are choking the peer
	am_choking: bool,
	/// Whether we are interested in the peer
	am_interested: bool,

	/// Whether the peer has choked us
	peer_choking: bool,
	/// Whether the peer is interested in us
	peer_interested: bool,

	/// The pieces the peer is currently serving
	peer_pieces: Option<Bitfield>,

	/// When the last message was sent by the client; in order to track keep alive messages
	last_message_sent: tokio::time::Instant,

	/// When the last message was received by the client; in order to terminate dead connections
	last_message_received: tokio::time::Instant
}

impl Peer {
	pub fn new(wire: Wire, handshake: Handshake) -> Peer {
		let id = WORKER_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

		Peer {
			id,
			wire,
			peer_id: handshake.peer_id,
			extensions: handshake.extensions,
			am_choking: true,
			am_interested: false,
			peer_choking: true,
			peer_interested: false,
			peer_pieces: None,
			// Last message sent & recieved are both now because initiating the connection counts
			last_message_sent: tokio::time::Instant::now(),
			last_message_received: tokio::time::Instant::now()
		}
	}

	pub async fn spawn(
		mut self,
		mode: task::Mode,
		torrent: TorrentPtr,
		event_tx: Sender<(WorkerId, PeerEvent)>,
		config: Arc<Configuration>
	) -> Result<(PeerHandle, impl Future<Output = Result<()>>)> {
		let (mode_tx, mode_rx) = tokio::sync::watch::channel(mode);

		self.peer_pieces = Some(Bitfield::new(torrent.meta_info.pieces.len()));

		let data = Arc::new(Mutex::new(self));

		let fut = task::run(config, torrent, data.clone(), event_tx, mode_rx);

		let handle = PeerHandle {
			data,
			_mode_tx: mode_tx
		};

		Ok((handle, fut))
	}

	/// Send a message
	pub async fn send(&mut self, message: Message) -> Result<()> {
		self.last_message_sent = tokio::time::Instant::now();

		match message {
			Message::Choke => self.am_choking = true,
			Message::Unchoke => self.am_choking = false,
			Message::Interested => self.am_interested = true,
			Message::NotInterested => self.am_interested = false,
			_ => ()
		};

		Ok(self.wire.send(message).await?)
	}

	/// Receive a message
	pub async fn receive(&mut self) -> Result<Message> {
		let message = self.wire.receive().await?;

		match &message {
			Message::Choke => self.peer_choking = true,
			Message::Unchoke => self.peer_choking = false,
			Message::Interested => self.peer_interested = true,
			Message::NotInterested => self.peer_interested = false,
			Message::Bitfield(data) => {
				let mut peer_pieces = self.peer_pieces.as_mut().unwrap();

				peer_pieces |= &mut Bitfield::from_bytes_length(data, peer_pieces.len()).unwrap();
			}
			Message::Have(i) => self.peer_pieces.as_mut().unwrap().set(*i as usize, true),
			_ => ()
		};

		Ok(message)
	}

	pub fn id(&self) -> WorkerId {
		self.id
	}

	pub fn peer_id(&self) -> &[u8; 20] {
		&self.peer_id
	}

	pub fn peer_id_short(&self) -> String {
		util::hex(&self.peer_id[16..20])
	}

	pub fn extensions(&self) -> &Extensions {
		&self.extensions
	}

	pub fn am_choking(&self) -> bool {
		self.am_choking
	}

	pub fn am_interested(&self) -> bool {
		self.am_interested
	}

	pub fn peer_choking(&self) -> bool {
		self.peer_choking
	}

	pub fn peer_interested(&self) -> bool {
		self.peer_interested
	}

	pub fn last_message_sent(&self) -> tokio::time::Instant {
		self.last_message_sent
	}

	pub fn last_message_received(&self) -> tokio::time::Instant {
		self.last_message_received
	}

	pub fn has_piece(&self, piece: u32) -> bool {
		// If we haven't received a bitfield, it means the peer has no pieces yet
		self.peer_pieces
			.as_ref()
			.map(|v| v.get(piece as usize))
			.unwrap_or(false)
	}
}

impl std::fmt::Display for Peer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"{} ({})",
			self.wire.peer_addr().map_err(|_| std::fmt::Error)?,
			PeerId::parse(&self.peer_id)
				.map(|peer_id| peer_id.to_string())
				.unwrap_or_else(|_| util::hex(&self.peer_id))
		)
	}
}
