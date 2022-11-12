use std::convert::TryInto;
use bytes::{BytesMut, Buf};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::message::{Message, MessageError};

const PROTOCOL: &[u8] = b"BitTorrent protocol";

#[derive(Error, Debug)]
pub enum WireError {
    #[error("A TCP/IP error occurred")]
    Network(#[from] std::io::Error),
    #[error("An invalid handshake was received")]
    InvalidHandshake,
    #[error("An invalid message was received")]
    InvalidMessage(#[from] MessageError),
}

pub(crate) type Result<T> = std::result::Result<T, WireError>;

pub struct Handshake {
    pub extensions: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

#[derive(Debug)]
pub struct Wire {
    stream: TcpStream, // BufWriter?
    buffer: BytesMut
}

impl Wire {
    /// Connect to a peer
    pub async fn connect<T: tokio::net::ToSocketAddrs>(addr: T) -> Result<Wire> {
        tokio::net::TcpStream::connect(addr)
            .await
            .map(Wire::new)
            .map_err(WireError::from)
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
    /// This method is not cancellation safe. If it is used as the event in a tokio::select statement and some other branch completes first, then the message may have been partially written, but future calls to send will start over from the beginning of the message.
    pub async fn send(&mut self, message: Message) -> Result<()> {
        self.send_raw(Vec::from(message).as_slice()).await
    }

    /// Receives a message from the peer
    ///
    /// # Cancel safety
    /// This method is cancel safe. If you use it as the event in a tokio::select statement and some other branch completes first, then it is guaranteed that no data was read.
    pub async fn receive(&mut self) -> Result<Message> {
        //let buf = Cursor::new(&self.buffer[..]);

        loop {
            if self.buffer.len() >= std::mem::size_of::<u32>() {
                let message_size = u32::from_be_bytes(self.buffer[0..4].try_into().unwrap()) as usize;
                let total_size = std::mem::size_of::<u32>() + message_size;

                if self.buffer.len() >= total_size {
                    let message = Message::try_from(&self.buffer[(total_size - message_size)..total_size])?;

                    self.buffer.advance(total_size);

                    return Ok(message);
                }
            }

            if self.stream.read_buf(&mut self.buffer).await? == 0 {
                // If read_buf returns zero it means the remote has closed the connection
                // ConnectionReset - Remote closed connection unexpectedly
                // UnexpectedEof - Remote closed connection while reading a message
                // ConnectionAborted

                return Err(WireError::Network(std::io::Error::from(
                    if self.buffer.is_empty() {
                        std::io::ErrorKind::ConnectionReset
                    } else {
                        std::io::ErrorKind::UnexpectedEof
                    }
                )))
            }
        }

        //Message::try_from(self.receive_raw().await?.as_slice())
    }

    pub async fn handshake(&mut self, handshake: &Handshake) -> Result<Handshake> {
        self.send_handshake(handshake).await?;
        let peer_handshake = self.receive_handshake().await?;

        if handshake.info_hash != peer_handshake.info_hash {
            Err(WireError::InvalidHandshake)
        } else {
            Ok(peer_handshake)
        }
    }

    /// Sends data over the wire
    #[deprecated = "not cancel safe"]
    async fn send_raw(&mut self, buffer: &[u8]) -> Result<()> {
        self.stream
            .write_u32(buffer.len().try_into().unwrap())
            .await?;
        self.stream.write_all(buffer).await?;

        Ok(())
    }

    /// Receives data over the wire
    #[deprecated = "not cancel safe"]
    async fn receive_raw(&mut self) -> Result<Vec<u8>> {
        let length = self.stream.read_u32().await?;
        let mut buffer = vec![0u8; length as usize];

        self.stream.read_exact(&mut buffer).await?;

        Ok(buffer)
    }

    /// Receives handshake info from the peer
    async fn receive_handshake(&mut self) -> Result<Handshake> {
        let protocol_length = self.stream.read_u8().await?;

        let mut protocol = vec![0u8; protocol_length as usize];
        self.stream.read_exact(protocol.as_mut_slice()).await?;

        if protocol != PROTOCOL {
            return Err(WireError::InvalidHandshake);
        }

        let mut extensions = [0u8; 8];
        self.stream.read_exact(&mut extensions).await?;

        let mut info_hash = [0u8; 20];
        self.stream.read_exact(&mut info_hash).await?;

        let mut peer_id = [0u8; 20];
        self.stream.read_exact(&mut peer_id).await?;

        Ok(Handshake {
            extensions,
            info_hash,
            peer_id,
        })
    }

    /// Sends the handshake info to the peer
    async fn send_handshake(&mut self, handshake: &Handshake) -> Result<()> {
        // Length prefixed protocol name
        self.stream
            .write_u8(PROTOCOL.len().try_into().unwrap())
            .await?;
        self.stream.write_all(PROTOCOL).await?;

        // Supported extensions. Currently, none are supported.
        self.stream.write_all(&handshake.extensions).await?;

        self.stream.write_all(&handshake.info_hash).await?;
        self.stream.write_all(&handshake.peer_id).await?;

        Ok(())
    }
}
