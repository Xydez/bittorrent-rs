use std::convert::TryInto;
use tokio::{net::TcpStream, io::{AsyncReadExt, AsyncWriteExt}};

use crate::bitfield::Bitfield;

const PROTOCOL: &str = "BitTorrent protocol";

#[derive(Debug)]
pub enum WireError {
	NetworkError(std::io::Error),
	InvalidHandshakeError,
	InvalidMessageError(String)
}

impl From<std::io::Error> for WireError {
	fn from(error: std::io::Error) -> Self {
		return WireError::NetworkError(error);
	}
}

impl std::fmt::Display for WireError {
	fn fmt(&self, _f: &mut std::fmt::Formatter) -> std::fmt::Result {
		todo!()
	}
}

impl std::error::Error for WireError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		todo!()
	}
}

pub(crate) type Result<T> = std::result::Result<T, WireError>;

pub struct Handshake {
	pub extensions: [u8; 8],
	pub info_hash: [u8; 20],
	pub peer_id: [u8; 20]
}

#[derive(Debug, Clone)]
pub enum Message {
	/// KeepAlive is a message of length zero to keep the connection alive
	KeepAlive,

	/// 'Choke' means that the peer serving the file is not currently accepting requests.
	Choke,

	/// 'Unchoke' means that the peer serving the file is currently accepting requests.
	Unchoke,

	/// ‘Interested’ means that the downloading client wants to download from the peer.
	Interested,

	/// ‘NotInterested’ means that the downloading client does not want to download from the peer.
	NotInterested,

	/// 'Have' means that the peer serving the file has started serving a piece.
	/// * index - Piece index
	Have(u32),

	/// 'Bifield' sends the pieces that the peer serving the file has initial access to. May only be sent directly after handshake.
	/// * bitfield - Pieces being served
	Bitfield(Bitfield),

	/// 'Request' requests a block from the peer serving the file.
	/// * index - Piece index, 
	/// * begin - Offset within piece measured in bytes
	/// * length - Block length, usually 16 384, sometimes truncated
	Request(u32, u32, u32),

	/// 'Piece' responds with the block that the downloading client has requested.
	/// * index - Piece index
	/// * begin - Offset within piece measured in bytes
	/// * piece - Raw bytes for the requested block
	Piece(u32, u32, Vec<u8>),

	/// 'Cancel' means that the downloading client has already received a block.
	/// * index - Piece index, 
	/// * begin - Offset within piece measured in bytes
	/// * length - Block length, usually 16 384, sometimes truncated
	Cancel(u32, u32, u32)
}

#[derive(Debug)]
pub struct Wire {
	stream: TcpStream
}

impl Wire {
	/// Initialize the peer wire
	pub fn new(stream: TcpStream) -> Wire {
		Wire {
			stream
		}
	}

	/// Receives a message from the peer
	pub async fn receive(&mut self) -> Result<Message> {
		// Messages of length zero are keepalives, and ignored. Keepalives are generally sent once every two minutes, but note that timeouts can be done much more quickly when data is expected.

		let buffer = self.receive_raw().await?;

		if buffer.len() == 0 {
			return Ok(Message::KeepAlive);
		}

		let id = buffer[0];
		// let id = u32::from_be_bytes(buffer[0..4].try_into().unwrap());
		let payload = &buffer[1..];

		match id {
			0..=3 => if payload.is_empty() {
				match id {
					0 => {
						Ok(Message::Choke)
					},
					1 => {
						Ok(Message::Unchoke)
					},
					2 => {
						Ok(Message::Interested)
					},
					3 => {
						Ok(Message::NotInterested)
					},
					_ => unreachable!()
				}
			} else {
				Err(WireError::InvalidMessageError("Message id 0-3 cannot have a payload".to_string()))
			},
			4 => Ok(Message::Have(u32::from_be_bytes(payload.try_into().map_err(|_| WireError::InvalidMessageError(format!("Message id 4 requires a payload of exactly 4 bytes (Got {})", payload.len())))?))),
			5 => {
				Ok(Message::Bitfield(Bitfield::from_bytes(payload)))
			},
			6 => if payload.len() == 3 * std::mem::size_of::<u32>() {
				Ok(Message::Request(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					u32::from_be_bytes(payload[8..12].try_into().unwrap())
				))
			} else {
				Err(WireError::InvalidMessageError(format!("Message id 6 requires a payload of exactly 12 bytes (Got {})", payload.len())))
			},
			7 => if payload.len() > 2 * std::mem::size_of::<u32>() {
				Ok(Message::Piece(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					payload[8..].to_vec()
				))
			} else {
				Err(WireError::InvalidMessageError(format!("Message id 7 requires a payload of more than 8 bytes (Got {})", payload.len())))
			},
			8 => if payload.len() == 3 * std::mem::size_of::<u32>() {
				Ok(Message::Cancel(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					u32::from_be_bytes(payload[8..12].try_into().unwrap())
				))
			} else {
				Err(WireError::InvalidMessageError(format!("Message id 8 requires a payload of exactly 12 bytes (Got {})", payload.len())))
			},
			id => Err(WireError::InvalidMessageError(format!("Message ids must be between 0-8 (Got {})", id)))
		}
	}

	/// Sends a message to the peer
	pub async fn send(&mut self, message: Message) -> Result<()> {
		let mut data = Vec::<u8>::new();

		let id = match message {
			Message::Choke => 0,
			Message::Unchoke => 1,
			Message::Interested => 2,
			Message::NotInterested => 3,
			Message::Have(..) => 4,
			Message::Bitfield(..) => 5,
			Message::Request(..) => 6,
			Message::Piece(..) => 7,
			Message::Cancel(..) => 8,
			Message::KeepAlive => {
				return self.stream.write_u32(0).await.map_err(|err| WireError::NetworkError(err));
			}
		};

		data.push(id);

		match message {
			Message::Have(index) => data.extend(index.to_be_bytes()),
			Message::Bitfield(bitfield) => data.extend(bitfield.as_bytes()),
			Message::Request(index, begin, length) => data.extend([index.to_be_bytes(), begin.to_be_bytes(), length.to_be_bytes()].concat()),
			Message::Piece(index, begin, piece) => data.extend([&index.to_be_bytes(), &begin.to_be_bytes(), piece.as_slice()].concat()),
			Message::Cancel(index, begin, length) => data.extend([index.to_be_bytes(), begin.to_be_bytes(), length.to_be_bytes()].concat()),
			_ => ()
		}

		self.send_raw(data.as_slice()).await
	}

	/// Sends data over the wire
	async fn send_raw(&mut self, buffer: &[u8]) -> Result<()> {
		self.stream.write_u32(buffer.len().try_into().unwrap()).await?;
		self.stream.write_all(buffer).await?;

		Ok(())
	}

	/// Receives data over the wire
	async fn receive_raw(&mut self) -> Result<Vec<u8>> {
		let length = self.stream.read_u32().await?;
		let mut buffer = vec![0u8; length as usize];

		self.stream.read_exact(&mut buffer).await?;

		Ok(buffer)
	}

	/// Receives handshake info from the peer
	pub async fn receive_handshake(&mut self) -> Result<Handshake> {
		let protocol_length = self.stream.read_u8().await?;

		let mut protocol = vec![0u8; protocol_length as usize];
		self.stream.read_exact(protocol.as_mut_slice()).await?;

		let protocol = String::from_utf8(protocol).map_err(|_| WireError::InvalidHandshakeError)?;

		if protocol != PROTOCOL {
			return Err(WireError::InvalidHandshakeError);
		}

		let mut extensions = [0u8; 8];
		self.stream.read_exact(&mut extensions).await?;

		let mut info_hash = [0u8; 20];
		self.stream.read_exact(&mut info_hash).await?;

		let mut peer_id = [0u8; 20];
		self.stream.read_exact(&mut peer_id).await?;

		return Ok(Handshake {
			extensions,
			info_hash,
			peer_id
		});
	}

	/// Sends the handshake info to the peer
	pub async fn send_handshake(&mut self, handshake: &Handshake) -> Result<()> {
		// Length prefixed protocol name
		self.stream.write_u8(PROTOCOL.len().try_into().unwrap()).await?;
		self.stream.write_all(PROTOCOL.as_bytes()).await?;

		// Supported extensions. Currently, none are supported.
		self.stream.write_all(&handshake.extensions).await?;

		self.stream.write_all(&handshake.info_hash).await?;
		self.stream.write_all(&handshake.peer_id).await?;

		return Ok(());
	}
}
