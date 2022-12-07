//! A block worker is responsible for downloading a block from a peer

use thiserror::Error;

use crate::{core::piece::PieceId, protocol::wire::message::Message};

#[derive(Error, Debug)]
pub enum Error {
	#[error("The worker has lagged {0} messages behind")]
	Lagged(u64),
	#[error("Choked during download")]
	Choked,
	#[error("Peer is shutting down")]
	Shutdown
}

pub type Result<T> = std::result::Result<T, Error>;

pub async fn get_block(
	pid: String,
	_permit: tokio::sync::OwnedSemaphorePermit,
	//piece_download: Arc<Mutex<PieceDownload>>,
	piece: PieceId,
	(block_begin, block_size): (u32, u32),
	message_tx: tokio::sync::mpsc::Sender<Message>,
	mut message_rx: tokio::sync::broadcast::Receiver<Message>
) -> Result<Vec<u8>> {
	log::trace!(
		"[{pid}] Requesting block {}:[{}-{}]",
		piece,
		block_begin,
		block_begin + block_size
	);
	message_tx
		.send(Message::Request(piece, block_begin, block_size))
		.await
		.unwrap();

	let result = loop {
		break match message_rx.recv().await {
			Ok(Message::Piece(index, begin, data))
				if index == piece && begin == block_begin && data.len() == block_size as usize =>
			{
				// We received the block
				log::trace!(
					"[{pid}] Received block {}:[{}-{}]",
					piece,
					block_begin,
					block_begin + block_size
				);

				Ok(data)
			}
			Ok(Message::Choke) => {
				log::warn!("[{pid}] choked during download");
				Err(Error::Choked)
			}
			Err(tokio::sync::broadcast::error::RecvError::Closed) => {
				log::trace!("[{pid}] get_block shutting down");
				Err(Error::Shutdown)
			}
			Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
				log::error!("[{pid}] get_block lagged behind {count} messages");
				Err(Error::Lagged(count))
			}
			_ => continue
		};
	};

	result
}