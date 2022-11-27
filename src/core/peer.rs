use thiserror::Error;

use crate::{
	core::bitfield::Bitfield,
	protocol::{
		extensions::Extensions,
		peer_id::PeerId,
		wire::{Handshake, Message, Wire, WireError}
	}
};

use super::{session::PieceID, util};

#[derive(Error, Debug)]
pub enum PeerError {
	#[error("An error occurred within the wire protocol.")]
	WireError(#[from] WireError)
}

pub(crate) type Result<T> = std::result::Result<T, PeerError>;

/// Abstraction of Wire that maintains peer state
#[derive(Debug)]
pub struct Peer {
	wire: Wire,
	peer_id: [u8; 20],

	// TODO: Maybe store as u64 or as an Extensions type.
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
	last_message_sent: tokio::time::Instant
}

impl Peer {
	/// Connects to a peer and perform a handshake
	pub async fn connect<T: tokio::net::ToSocketAddrs>(
		addr: T,
		handshake: Handshake
	) -> std::io::Result<Result<Peer>> {
		Ok(Peer::handshake(Wire::connect(addr).await?, handshake).await)
	}

	/// Performs a handshake with a peer
	pub async fn handshake(mut wire: Wire, handshake: Handshake) -> Result<Peer> {
		let peer_handshake = wire.handshake(&handshake).await?;

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
			last_message_sent: tokio::time::Instant::now()
		})
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
			Message::Bitfield(data) => self.peer_pieces = Some(data.clone()),
			Message::Have(i) => self.peer_pieces.as_mut().unwrap().set(*i as usize, true),
			_ => ()
		};

		Ok(message)
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

	pub fn has_piece(&self, i: PieceID) -> bool {
		// If we haven't received a bitfield, it means the peer has no pieces yet
		self.peer_pieces
			.as_ref()
			.map(|v| v.get(i as usize))
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
