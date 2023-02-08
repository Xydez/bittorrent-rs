use std::{convert::TryInto, net::SocketAddr};

use bytes::{Buf, BytesMut};
use thiserror::Error;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpStream
};

use crate::{
	core::configuration::Configuration,
	protocol::{
		extensions::Extensions,
		wire::message::{self, Message}
	}
};

const PROTOCOL: &[u8] = b"BitTorrent protocol";

#[derive(Error, Debug)]
pub enum Error {
	#[error("A TCP/IP error occurred")]
	Network(#[from] std::io::Error),
	#[error("An invalid handshake was received")]
	InvalidHandshake,
	#[error("An invalid message was received")]
	InvalidMessage(#[from] message::Error)
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Handshake {
	pub extensions: Extensions,
	pub info_hash: [u8; 20],
	pub peer_id: [u8; 20]
}

impl Handshake {
	pub fn new(info_hash: [u8; 20], config: &Configuration) -> Handshake {
		Handshake {
			extensions: config.extensions,
			info_hash,
			peer_id: config.peer_id
		}
	}
}

#[derive(Debug)]
pub struct Wire {
	stream: TcpStream,
	buffer: BytesMut
}

impl Wire {
	/// Connect to a peer
	pub async fn connect<T: tokio::net::ToSocketAddrs>(addr: T) -> std::io::Result<Wire> {
		tokio::net::TcpStream::connect(addr).await.map(Wire::new)
	}

	/// Initialize the peer wire
	pub fn new(stream: TcpStream) -> Wire {
		Wire {
			stream,
			buffer: BytesMut::with_capacity(32_768) // 32 KiB
		}
	}

	/// Sends a message to the peer
	///
	/// # Cancel safety
	/// This method IS NOT cancel safe
	pub async fn send(&mut self, message: Message) -> Result<()> {
		let buffer = Vec::from(message);

		self.stream.write_u32(buffer.len() as u32).await?;
		self.stream.write_all(&buffer).await?;

		Ok(())
	}

	/// Receives a message from the peer
	///
	/// # Cancel safety
	/// This method IS cancel safe
	pub async fn receive(&mut self) -> Result<Message> {
		loop {
			if self.buffer.len() >= std::mem::size_of::<u32>() {
				let message_size =
					u32::from_be_bytes(self.buffer[0..4].try_into().unwrap()) as usize;
				let total_size = std::mem::size_of::<u32>() + message_size;

				if self.buffer.len() >= total_size {
					let message =
						Message::try_from(&self.buffer[(total_size - message_size)..total_size])?;

					self.buffer.advance(total_size);

					return Ok(message);
				}
			}

			if self.stream.read_buf(&mut self.buffer).await? == 0 {
				// If read_buf returns zero it means the remote has closed the connection

				return Err(Error::Network(std::io::Error::from(
					if self.buffer.is_empty() {
						std::io::ErrorKind::ConnectionReset
					} else {
						std::io::ErrorKind::UnexpectedEof
					}
				)));
			}
		}
	}

	/// Exchanges a hanshake with the peer
	///
	/// # Cancel safety
	/// This method IS NOT cancel safe
	#[deprecated = "Use either receive_handshake or send_handshake depending on the initiator"]
	pub async fn handshake(&mut self, handshake: &Handshake) -> Result<Handshake> {
		self.send_handshake(handshake).await?;
		let peer_handshake = self.receive_handshake().await?;

		if handshake.info_hash != peer_handshake.info_hash {
			Err(Error::InvalidHandshake)
		} else {
			Ok(peer_handshake)
		}
	}

	/// Returns the socket address of the remote peer of this TCP connection.
	pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
		self.stream.peer_addr()
	}

	/// Receives handshake info from the peer
	///
	/// # Cancel safety
	/// This method IS NOT cancel safe
	pub async fn receive_handshake(&mut self) -> Result<Handshake> {
		let protocol_length = self.stream.read_u8().await?;

		let mut protocol = vec![0u8; protocol_length as usize];
		self.stream.read_exact(protocol.as_mut_slice()).await?;

		if protocol != PROTOCOL {
			return Err(Error::InvalidHandshake);
		}

		let mut extensions = [0u8; 8];
		self.stream.read_exact(&mut extensions).await?;

		let extensions = Extensions(extensions);

		let mut info_hash = [0u8; 20];
		self.stream.read_exact(&mut info_hash).await?;

		let mut peer_id = [0u8; 20];
		self.stream.read_exact(&mut peer_id).await?;

		Ok(Handshake {
			extensions,
			info_hash,
			peer_id
		})
	}

	/// Sends the handshake info to the peer
	///
	/// # Cancel safety
	/// This method IS NOT cancel safe
	pub async fn send_handshake(&mut self, handshake: &Handshake) -> Result<()> {
		// Length prefixed protocol name
		self.stream
			.write_u8(PROTOCOL.len().try_into().unwrap())
			.await?;
		self.stream.write_all(PROTOCOL).await?;

		self.stream.write_all(&handshake.extensions.0).await?;

		self.stream.write_all(&handshake.info_hash).await?;
		self.stream.write_all(&handshake.peer_id).await?;

		Ok(())
	}
}
