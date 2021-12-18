use std::{convert::TryInto};

use tokio::{net::{TcpStream}, io::{AsyncWriteExt, AsyncReadExt}};

use crate::{metainfo::MetaInfo, bitfield::Bitfield};

#[derive(Debug)]
pub enum PeerError {
	IOError(std::io::Error),
	BencodeError(serde_bencode::Error),
	StringEncodeError(std::string::FromUtf8Error),
	InvalidProtocolError,
	InvalidMessageError
}

impl From<std::io::Error> for PeerError {
	fn from(error: std::io::Error) -> Self {
		return PeerError::IOError(error);
	}
}

impl From<serde_bencode::Error> for PeerError {
	fn from(error: serde_bencode::Error) -> Self {
		return PeerError::BencodeError(error);
	}
}

impl From<std::string::FromUtf8Error> for PeerError {
	fn from(error: std::string::FromUtf8Error) -> Self {
		return PeerError::StringEncodeError(error);
	}
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

pub(crate) type Result<T> = std::result::Result<T, PeerError>;

#[derive(Debug)]
pub struct Peer {
	stream: TcpStream,
	peer_id: [u8; 20],
	choked: bool,
	interested: bool
}

struct Handshake {
	extensions: [u8; 8],
	info_hash: [u8; 20],
	peer_id: [u8; 20]
}

/// Messages that can be sent over the BitTorrent wire protocol.
/// * 0 - Choke
/// * 1 - Unchoke
/// * 2 - Interested
/// * 3 - NotInterested
/// * 4 - Have
/// * 5 - Bitfield
/// * 6 - Request
/// * 7 - Piece
/// * 8 - Cancel
pub enum Message {
	/// 'Choke' means that the peer serving the file is not currently accepting requests.
	Choke,
	/// 'Unchoke' means that the peer serving the file is currently accepting requests.
	Unchoke,
	/// ‘Interested’ means that the downloading client wants to download from the peer.
	Interested,
	/// ‘NotInterested’ means that the downloading client does not want to download from the peer.
	NotInterested,

	/// 'Have' means that the peer serving the file has started serving a piece.
	Have(u32),
	/// 'Bifield' sends the pieces that the peer serving the file has initial access to. Must be sent directly after handshake.
	Bitfield(Bitfield),

	/// 'Request' requests a block from the peer serving the file.
	/// * index (Piece index), 
	/// * begin (Offset within piece measured in bytes)
	/// * length (Block length, usually 16 384, sometimes truncated)
	Request(u32, u32, u32),

	/// 'Piece' responds with the block that the downloading client has requested.
	/// * index (Piece index)
	/// * begin (Offset within piece measured in bytes)
	/// * piece (Raw bytes for the requested block)
	Piece(u32, u32, Vec<u8>),

	/// 'Cancel' means that the downloading client has already received a block.
	/// * index (Piece index), 
	/// * begin (Offset within piece measured in bytes)
	/// * length (Block length, usually 16 384, sometimes truncated)
	Cancel(u32, u32, u32)
}

const PROTOCOL: &str = "BitTorrent protocol";

impl Peer {
	/// Makes a TCP connection to a peer and complete a BitTorrent handshake
	pub async fn connect(addr: std::net::SocketAddrV4, meta: &MetaInfo, peer_id: &[u8; 20]) -> Result<Peer> {
		let mut stream = TcpStream::connect(addr).await?;

		Peer::send_handshake(&mut stream, Handshake {
			extensions: [0; 8],
			info_hash: meta.info_hash,
			peer_id: *peer_id
		}).await?;

		let handshake = Peer::receive_handshake(&mut stream).await?;

		Ok(Peer {
			stream,
			peer_id: handshake.peer_id,
			choked: true,
			interested: false
		})
	}

	/// Sends the handshake info to the peer
	async fn send_handshake(stream: &mut TcpStream, handshake: Handshake) -> Result<()> {
		// Character 19 followed by 'BitTorrent protocol'
		stream.write_u8(PROTOCOL.len().try_into().unwrap()).await?;
		stream.write_all(PROTOCOL.as_bytes()).await?;

		// Supported extensions. Currently, none are supported.
		stream.write_all(&handshake.extensions).await?;

		stream.write_all(&handshake.info_hash).await?;
		stream.write_all(&handshake.peer_id).await?;

		return Ok(());
	}

	/// Receives handshake info from the peer
	async fn receive_handshake(stream: &mut TcpStream) -> Result<Handshake> {
		let protocol_length = stream.read_u8().await?;

		let mut protocol = vec![0u8; protocol_length as usize];
		stream.read_exact(protocol.as_mut_slice()).await?;

		let protocol = String::from_utf8(protocol)?;

		if protocol != PROTOCOL {
			return Err(PeerError::InvalidProtocolError);
		}

		let mut extensions = [0u8; 8];
		stream.read_exact(&mut extensions).await?;

		let mut info_hash = [0u8; 20];
		stream.read_exact(&mut info_hash).await?;

		let mut peer_id = [0u8; 20];
		stream.read_exact(&mut peer_id).await?;

		return Ok(Handshake {
			extensions,
			info_hash,
			peer_id
		});
	}
	
	/// Receives a message from the peer
	pub async fn receive_message(&mut self) -> Result<Message> {
		let buffer = self.receive_raw().await?;
		let id = u32::from_be_bytes(buffer[0..4].try_into().unwrap());
		let payload = &buffer[4..];

		match id {
			0..=3 => if payload.is_empty() {
				match id {
					0 => Ok(Message::Choke),
					1 => Ok(Message::Unchoke),
					2 => Ok(Message::Interested),
					3 => Ok(Message::NotInterested),
					_ => Err(PeerError::InvalidMessageError)
				}
			} else {
					Err(PeerError::InvalidMessageError)
			},
			4 => Ok(Message::Have(u32::from_be_bytes(payload.try_into().map_err(|_| PeerError::InvalidMessageError)?))),
			5 => Ok(Message::Bitfield(Bitfield::from_bytes(payload.to_vec()))),
			6 => if payload.len() == 3 * std::mem::size_of::<u32>() {
				Ok(Message::Request(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					u32::from_be_bytes(payload[8..12].try_into().unwrap())
				))
			} else {
				Err(PeerError::InvalidMessageError)
			},
			7 => if payload.len() > 2 * std::mem::size_of::<u32>() {
				Ok(Message::Piece(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					payload[8..].to_vec()
				))
			} else {
				Err(PeerError::InvalidMessageError)
			},
			8 => if payload.len() == 3 * std::mem::size_of::<u32>() {
				Ok(Message::Cancel(
					u32::from_be_bytes(payload[0..4].try_into().unwrap()),
					u32::from_be_bytes(payload[4..8].try_into().unwrap()),
					u32::from_be_bytes(payload[8..12].try_into().unwrap())
				))
			} else {
				Err(PeerError::InvalidMessageError)
			},
			_ => Err(PeerError::InvalidMessageError)
		}
	}

	/// Sends a message to the peer
	pub async fn send_message(&mut self, message: Message) -> Result<()> {
		todo!()
	}

	async fn receive_raw(&mut self) -> Result<Vec<u8>> {
		let length = self.stream.read_u32().await?;
		let mut buffer = vec![0u8; length as usize];

		self.stream.read_exact(&mut buffer).await?;

		Ok(buffer)
	}

	async fn send_raw(&mut self, buffer: Vec<u8>) -> Result<()> {
		self.stream.write_u32(buffer.len().try_into().unwrap()).await?;
		self.stream.write_all(buffer.as_slice()).await?;

		Ok(())
	}
}
