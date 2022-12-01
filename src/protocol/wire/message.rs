use thiserror::Error;

use crate::core::bitfield::Bitfield;

#[derive(Error, Debug)]
pub enum MessageError {
	#[error("Message ids must be between 0-8 (Got {0})")]
	InvalidID(u8),
	#[error("Invalid payload length")]
	InvalidPayload
}

pub(crate) type Result<T> = std::result::Result<T, MessageError>;

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

impl Message {
	fn id(&self) -> Option<u8> {
		match self {
			Message::Choke => Some(0),
			Message::Unchoke => Some(1),
			Message::Interested => Some(2),
			Message::NotInterested => Some(3),
			Message::Have(..) => Some(4),
			Message::Bitfield(..) => Some(5),
			Message::Request(..) => Some(6),
			Message::Piece(..) => Some(7),
			Message::Cancel(..) => Some(8),
			Message::KeepAlive => None
		}
	}
}

impl TryFrom<&[u8]> for Message {
	type Error = MessageError;

	fn try_from(buffer: &[u8]) -> Result<Self> {
		// Messages of length zero are keepalives, and ignored. Keepalives are generally sent once every two minutes, but note that timeouts can be done much more quickly when data is expected.

		if buffer.is_empty() {
			return Ok(Message::KeepAlive);
		}

		let id = buffer[0];
		let payload = &buffer[1..];

		match id {
			0..=3 => {
				if payload.is_empty() {
					match id {
						0 => Ok(Message::Choke),
						1 => Ok(Message::Unchoke),
						2 => Ok(Message::Interested),
						3 => Ok(Message::NotInterested),
						_ => unreachable!()
					}
				} else {
					Err(MessageError::InvalidPayload)
				}
			}
			4 => Ok(Message::Have(u32::from_be_bytes(
				payload
					.try_into()
					.map_err(|_| MessageError::InvalidPayload)?
			))),
			5 => Ok(Message::Bitfield(Bitfield::from_bytes(payload))),
			6 => {
				if payload.len() == 3 * std::mem::size_of::<u32>() {
					Ok(Message::Request(
						u32::from_be_bytes(payload[0..4].try_into().unwrap()),
						u32::from_be_bytes(payload[4..8].try_into().unwrap()),
						u32::from_be_bytes(payload[8..12].try_into().unwrap())
					))
				} else {
					Err(MessageError::InvalidPayload)
				}
			}
			7 => {
				if payload.len() > 2 * std::mem::size_of::<u32>() {
					Ok(Message::Piece(
						u32::from_be_bytes(payload[0..4].try_into().unwrap()),
						u32::from_be_bytes(payload[4..8].try_into().unwrap()),
						payload[8..].to_vec()
					))
				} else {
					Err(MessageError::InvalidPayload)
				}
			}
			8 => {
				if payload.len() == 3 * std::mem::size_of::<u32>() {
					Ok(Message::Cancel(
						u32::from_be_bytes(payload[0..4].try_into().unwrap()),
						u32::from_be_bytes(payload[4..8].try_into().unwrap()),
						u32::from_be_bytes(payload[8..12].try_into().unwrap())
					))
				} else {
					Err(MessageError::InvalidPayload)
				}
			}
			id => Err(MessageError::InvalidID(id))
		}
	}
}

impl From<Message> for Vec<u8> {
	fn from(message: Message) -> Self {
		let mut data = Vec::new();

		if let Some(id) = message.id() {
			data.push(id);
		}

		match message {
			Message::Have(index) => data.extend(index.to_be_bytes()),
			Message::Bitfield(bitfield) => data.extend(bitfield.as_bytes()),
			Message::Request(index, begin, length) => data.extend(
				[
					index.to_be_bytes(),
					begin.to_be_bytes(),
					length.to_be_bytes()
				]
				.concat()
			),
			Message::Piece(index, begin, piece) => {
				data.extend([&index.to_be_bytes(), &begin.to_be_bytes(), piece.as_slice()].concat())
			}
			Message::Cancel(index, begin, length) => data.extend(
				[
					index.to_be_bytes(),
					begin.to_be_bytes(),
					length.to_be_bytes()
				]
				.concat()
			),
			_ => ()
		}

		data
	}
}

impl std::fmt::Display for Message {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Message::KeepAlive => write!(f, "KeepAlive"),
			Message::Choke => write!(f, "KeepAlive"),
			Message::Unchoke => write!(f, "KeepAlive"),
			Message::Interested => write!(f, "KeepAlive"),
			Message::NotInterested => write!(f, "KeepAlive"),
			Message::Have(i) => write!(f, "Have({})", i),
			Message::Bitfield(bitfield) => write!(f, "Bitfield(<{} elements>)", bitfield.len()),
			Message::Request(index, begin, length) => {
				write!(f, "Request({}, {}, {})", index, begin, length)
			}
			Message::Piece(index, begin, piece) => {
				write!(f, "Piece({}, {}, <{} elements>)", index, begin, piece.len())
			}
			Message::Cancel(index, begin, length) => {
				write!(f, "Cancel({}, {}, {})", index, begin, length)
			}
		}
	}
}
