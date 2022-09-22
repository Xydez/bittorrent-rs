use crate::{
	core::bitfield::Bitfield,
	protocol::wire::{Handshake, Message, Wire, WireError}
};

// TODO: I'm thinking something like a task queue, so basically we have:
// (== for the user ==)
// let peer: Arc<Mutex<Peer>>;
// peer.receive_piece().await?
// (== internally ==)
// async fn receive_piece() {
//     let (notify_rx, notify_tx): ?; // Something that can be notified kind of
//     task_tx.send((notify_tx, TaskRequest::ReceivePiece));
//     return notify_rx.await?;
// }
// (== in some async task ==)
// let task_rx: ?; // Some kind of message thingy with iter
// for (notify, task) in queue {
//     match task {
//         TaskRequest::ReceivePiece => {
//             (...)
//             notify.notify(TaskResponse::PieceRecieved(...))
//         }
//     }
// }

/*
let (task_tx, task_rx) = tokio::sync::mpsc::unbounded_channel();
task.tx

*/

#[derive(Debug)]
pub enum PeerError {
	/// The peer sent an illegal handshake
	InvalidHandshake,
	/// The peer wire connection failed. Either an invalid message was sent or the connection stopped working.
	WireError(WireError),
	/// An error occurred with the connection
	NetworkError(std::io::Error)
}

impl std::fmt::Display for PeerError {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		match self {
			PeerError::InvalidHandshake => write!(f, "Invalid handshake"),
			PeerError::WireError(error) => write!(f, "Wire error: {}", error),
			PeerError::NetworkError(error) => write!(f, "Network error: {}", error)
		}
	}
}

impl From<WireError> for PeerError {
	fn from(error: WireError) -> Self {
		PeerError::WireError(error)
	}
}

pub(crate) type Result<T> = std::result::Result<T, PeerError>;

pub trait MessageCallback = Fn(&Peer, &Message);

/// Abstraction of Wire that maintains peer state
pub struct Peer {
	wire: Wire,
	peer_id: [u8; 20],
	// TODO: HOWWW???

	// TODO: Maybe store as u64 or as an Extensions type.
	/// The extensions the peer supports
	extensions: [u8; 8],

	/// Whether we are choking the peer
	am_choking: bool,
	/// Whether we are interested in the peer
	am_interested: bool,

	/// Whether the peer has choked us
	peer_choking: bool,
	/// Whether the peer is interested in us
	peer_interested: bool,

	/// The pieces the peer is currently serving
	peer_pieces: Option<Bitfield>
}

impl Peer {
	pub async fn connect<T: tokio::net::ToSocketAddrs>(
		addr: T,
		handshake: Handshake
	) -> Result<Peer> {
		Peer::new(Wire::connect(addr).await?, handshake).await
	}

	/// Connects to a peer and sends a handshake
	pub async fn new(mut wire: Wire, handshake: Handshake) -> Result<Peer> {
		wire.send_handshake(&handshake).await?;
		let peer_handshake = wire.receive_handshake().await?;

		if handshake.info_hash != peer_handshake.info_hash {
			return Err(PeerError::InvalidHandshake);
		}

		// TODO: If the initiator of the connection receives a handshake in which the peer_id does not match the expected peer_id, then the initiator is expected to drop the connection. Note that the initiator presumably received the peer information from the tracker, which includes the peer_id that was registered by the peer. The peer_id from the tracker and in the handshake are expected to match.

		Ok(Peer {
			wire,
			peer_id: peer_handshake.peer_id,
			extensions: peer_handshake.extensions,
			am_choking: true,
			am_interested: false,
			peer_choking: true,
			peer_interested: false,
			peer_pieces: None,
		})
	}

	pub fn send(&mut self, message: Message) -> Result<()> {
		match message {
			Message::Choke => self.am_choking = true,
			Message::Unchoke => self.am_choking = false,
			Message::Interested => self.am_interested = true,
			Message::NotInterested => self.am_interested = false,
			_ => ()
		};

		Ok(self.wire.send(message).await?)
	}

	async fn receive(&mut self) -> Result<Message> {
		let message = self.wire.receive().await?;

		match &message {
			Message::Choke => self.peer_choking = true,
			Message::Unchoke => self.peer_choking = false,
			Message::Interested => self.peer_interested = true,
			Message::NotInterested => self.peer_interested = false,
			Message::Bitfield(data) => self.peer_pieces = Some(data.clone()),
			Message::Have(i) => self.peer_pieces.as_mut().unwrap().set(*i as usize, true),
			_ => ()
		};

		Ok(message)
	}

	pub fn peer_id(&self) -> &[u8; 20] {
		&self.peer_id
	}

	pub fn extensions(&self) -> &[u8; 8] {
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

	pub fn has_piece(&self, i: usize) -> bool {
		// If we haven't received a bitfield, it means the peer has no pieces yet
		self.peer_pieces.as_ref().map(|v| v.get(i)).unwrap_or(false)
	}
}

// When the peer is dropped, tell the receive task to stop listening to the peer
// self.task.abort();
