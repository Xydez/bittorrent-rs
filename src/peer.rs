use crate::{
	bitfield::Bitfield,
	wire::{Handshake, Message, Wire, WireError}
};

use tokio::net::TcpStream;

#[derive(Debug)]
pub enum PeerError {
	NetworkError,
	InvalidHandshake,
	WireError(WireError)
}

impl std::fmt::Display for PeerError {
	fn fmt(&self, _f: &mut std::fmt::Formatter) -> std::fmt::Result {
		todo!()
	}
}

impl std::error::Error for PeerError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		todo!()
	}
}

impl From<WireError> for PeerError {
	fn from(error: WireError) -> Self {
		PeerError::WireError(error)
	}
}

pub(crate) type Result<T> = std::result::Result<T, PeerError>;

#[derive(Debug)]
pub struct Peer {
	wire: Wire,

	/// Whether we are choking the peer
	choking: bool,
	/// Whether we are interested in the peer
	interested: bool,

	/// Whether the peer has choked us
	peer_choking: bool,
	/// Whether the peer is interested in us
	peer_interested: bool,

	/// The pieces the peer is currently serving
	peer_pieces: Option<Bitfield>
}

// Idea: Peer is basically Wire but with state

impl Peer {
	pub async fn handshake(stream: TcpStream, handshake: Handshake) -> Result<Peer> {
		let mut wire = Wire::new(stream);

		wire.send_handshake(&handshake).await?;
		let peer_handshake = wire.receive_handshake().await?;

		if handshake.info_hash != peer_handshake.info_hash {
			return Err(PeerError::InvalidHandshake);
		}

		Ok(Peer {
			wire,
			choking: true,
			interested: false,
			peer_choking: true,
			peer_interested: false,
			peer_pieces: None
		})
	}

	pub async fn send(&mut self, message: Message) -> Result<()> {
		match message {
			Message::Choke => self.choking = true,
			Message::Unchoke => self.choking = false,
			Message::Interested => self.interested = true,
			Message::NotInterested => self.interested = false,
			_ => ()
		};

		Ok(self.wire.send(message).await?)
	}

	pub async fn receive(&mut self) -> Result<Message> {
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

	pub fn choking(&self) -> bool {
		self.choking
	}

	pub fn interested(&self) -> bool {
		self.interested
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
