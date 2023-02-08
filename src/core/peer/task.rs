use std::sync::Arc;

use tokio::sync::Mutex;

use super::Peer;
use crate::{
	core::{
		algorithm,
		bitfield::Bitfield,
		configuration::Configuration,
		event::{PeerEvent, Sender},
		peer::{
			block_worker::{self, get_block},
			Error, Result
		},
		piece::{self, PieceId},
		piece_download::PieceDownload,
		session::{PeerPtr, TorrentPtr},
		torrent::{TorrentLock, WorkerId},
		util
	},
	protocol::wire::message::Message
};

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct Mode {
	pub download: bool,
	pub seed: bool
}

/*
pub struct State {
	pub pending_blocks: HashSet<(u32, usize)>
}
*/

async fn get_download(
	torrent: &mut TorrentLock<'_>,
	peer: &Peer,
	pid: &str,
	config: &Configuration,
	current_download: &mut Option<(PieceId, Arc<Mutex<PieceDownload>>)>
) -> Option<(PieceId, Arc<Mutex<PieceDownload>>)> {
	// 1. Try to continue on the current download
	if let Some((piece, download)) = current_download {
		if download
			.lock()
			.await
			.pending_blocks()
			.peekable()
			.peek()
			.is_some()
		{
			return Some((*piece, download.clone()));
		}
	}

	// 2. If we can't continue on the current download, select a new piece
	let Some(piece) = algorithm::select_piece(torrent, peer, config) else {
		//log::debug!("[{pid}] no pieces to download");
		//maybe_blocks = false;
		//continue;
		return None;
	};

	let piece_size = util::piece_size(piece, &torrent.torrent.meta_info);

	let downloads_len = torrent.state().downloads.len();

	// 3. Get the PieceDownload
	let download = torrent
		.state_mut()
		.downloads
		.entry(piece)
		.or_insert_with(|| {
			log::info!("[{pid}] Creating download, {downloads_len} concurrent piece downloads");

			Arc::new(Mutex::new(PieceDownload::new(config, piece_size)))
		})
		.clone();

	current_download.replace((piece, download.clone()));

	Some((piece, download))
}

pub async fn run(
	config: Arc<Configuration>,
	torrent: TorrentPtr,
	peer: PeerPtr,
	event_tx: Sender<(WorkerId, PeerEvent)>,
	mut mode_rx: tokio::sync::watch::Receiver<Mode>
) -> Result<()> {
	/* Variables */

	let (message_send_tx, mut message_send_rx) = tokio::sync::mpsc::channel::<Message>(128);
	let (message_recv_tx, _) = tokio::sync::broadcast::channel::<Message>(128);

	let pid = peer.lock().await.peer_id_short();

	let block_semaphore = Arc::new(tokio::sync::Semaphore::new(
		config.concurrent_block_downloads
	));

	// Keeps track of the current piece download
	//let mut picker_iter = PieceIterator::default();
	let mut block_tasks = tokio_util::task::JoinMap::new();
	// TODO: Track the current block tasks in a HashSet as well so it can be
	//       accessed by users? Figure out how to get the peer task's state.

	// True if we need to check whether there are downloadable blocks in the current download
	// False until we receive a bitfield from the peer
	let mut maybe_blocks = false;

	// True if no messages have been received from the peer yet
	let mut first_message = true;

	let mut current_download = None;

	/* Initialize */
	// Create a bitfield of our pieces
	let bitfield = {
		let torrent = torrent.lock().await;

		Bitfield::from_bytes_length(
			&torrent
				.state()
				.pieces
				.chunks(8)
				.map(|pieces| {
					pieces.iter().enumerate().fold(0u8, |acc, (i, piece)| {
						if piece.state == piece::State::Done {
							acc + (1 << (7 - i))
						} else {
							acc
						}
					})
				})
				.collect::<Vec<u8>>(),
			torrent.state().pieces.len()
		)
		.unwrap()
	};

	{
		let mut peer = peer.lock().await;

		// Send bitfield on start
		log::debug!("[{pid}] Sending bitfield to peer");
		peer.send(Message::Bitfield(bitfield.as_bytes().to_vec()))
			.await
			.unwrap();

		// Send interested/unchoke messages on start
		let mode = *mode_rx.borrow_and_update();

		if mode.download {
			log::debug!("[{pid}] Sending interested message to peer");
			peer.send(Message::Interested).await.unwrap();
		}

		/*
		if mode.seed {
			log::debug!("[{pid}] Sending Message::Unchoke");
			peer.send(Message::Unchoke).await.unwrap();
		}
		*/
	}

	loop {
		let mut peer = peer.lock().await;
		let last_message_sent = peer.last_message_sent();

		tokio::select! {
			// Forward messages from message_send
			message = message_send_rx.recv() => {
				let message = message.unwrap();

				log::trace!("[{pid}] Forwarding message from block task: {}", message);
				peer.send(message).await?;
			},
			// Forward messages to message_recv
			message = peer.receive() => {
				let message = match message {
					Ok(message) => message,
					Err(error) => {
						log::error!("[{pid}] Failed to receive message: {}", util::error_chain(&error));
						return Err(error);
					}
				};

				if !first_message && matches!(&message, Message::Bitfield(_)) {
					log::error!("Invalid bitfield");
					return Err(Error::IllegalMessage(message));
				}

				if let Message::Have(_) | Message::Bitfield(_) = message {
					// If we receive a have/bitfield message it means there is a new piece for us to download.
					// TODO: Compare against our own bitfield to see if there are pieces to download
					maybe_blocks = true;
				}

				log::trace!("[{pid}] Forwarding message to block tasks: {}", message);

				if message_recv_tx.send(message).is_err() {
					log::trace!("[{pid}] no message receivers");
				}

				first_message = false;
			},
			// Keep the peer connection alive
			_ = tokio::time::sleep_until(last_message_sent + config.alive_timeout) => {
				peer.send(Message::KeepAlive).await?;
			}
			// If downloading is enabled, we are unchoked and there are blocks available, download them
			permit = block_semaphore.clone().acquire_owned(), if !peer.peer_choking() && mode_rx.borrow().download && maybe_blocks => {
				let (download, piece, block) = {
					let mut torrent = torrent.lock().await;

					// TODO: Continue here!
					let Some((piece, download)) = get_download(&mut torrent, &peer, &pid, &config, &mut current_download).await else {
						log::debug!("[{pid}] no pieces to download");
						maybe_blocks = false;
						continue;
					};

					let Some(block) = algorithm::select_block(&*download.lock().await, peer.id()) else {
						log::debug!("[{pid}] no blocks to download");
						maybe_blocks = false;
						continue;
					};

					(download, piece, block)
				};

				let block_region = {
					let mut download = download.lock().await;
					download.block_downloads.push_back((block, peer.id()));

					let block = &mut download.blocks[block];

					(block.begin, block.size)
				};

				log::trace!("[{pid}] starting get_block");
				block_tasks.spawn(
					(piece, block),
					get_block(
						pid.clone(),
						permit.unwrap(),
						piece,
						block_region,
						message_send_tx.clone(),
						message_recv_tx.subscribe(),
					)
				);
			},
			// Receive the data from completed get_block tasks
			Some(((piece, block), result)) = block_tasks.join_next(), if !block_tasks.is_empty() => {
				// TODO: An error sometimes occurs here
				// thread 'main' panicked at 'no entry found for key', src\core\peer\task.rs:274:32
				// ^ I'm suspecting this is because we are selecting duplicate
				//   blocks or something, look over the current piece/block
				//   selection algorithm.

				let Some(download) = torrent.lock().await.state().downloads.get(&piece).cloned() else {
					log::warn!("[{pid}] Received duplicate block {} (on nonexistent download)", util::fmt_block(piece, block));
					continue;
				};

				match result.unwrap() {
					Ok(data) => {
						let mut lock = download.lock().await;

						if lock.blocks[block].data.is_none() {
							lock.blocks[block].data = Some(data.into());

							let blocks_total = lock.blocks().count();
							let blocks_done = lock.blocks().filter(|block| block.data.is_some()).count();

							//let block_begin = lock.blocks[block].begin;
							//let block_size = lock.blocks[block].size;

							log::trace!(
								"[{pid}] Received block {} ({}/{} blocks)",
								util::fmt_block(piece, block),
								blocks_done, blocks_total
							);

							event_tx.send((peer.id(), PeerEvent::BlockReceived(piece, block))).unwrap();
						} else {
							log::warn!("[{pid}] Received duplicate block {}", util::fmt_block(piece, block));
						}
					},
					Err(block_worker::Error::Choked) => {
						log::warn!("[{pid}] Choked during download");
					},
					Err(err) => {
						log::error!("[{pid}] get_blocked errored: {}", util::error_chain(err));
					}
				}
			},
			// Respond to changes in the worker's mode
			result = mode_rx.changed() => {
				if result.is_err() {
					// The session has shut down and we should exit gracefully
					log::debug!("[{pid}] Worker channel closed, exiting...");
					break Ok(());
				} else {
					let mode = *mode_rx.borrow();

					if !mode.download && peer.am_interested() {
						// We entered download mode and should inform the peer
						log::debug!("[{pid}] Sending Message::Interested");
						peer.send(Message::Interested).await.unwrap();
					} else if mode.download && !peer.am_interested() {
						// We exited download mode and should inform the peer
						log::debug!("[{pid}] Sending Message::NotInterested");
						peer.send(Message::NotInterested).await.unwrap();

						// TODO: Send Message::Cancel, either here or when calling join_next
						block_tasks.shutdown().await;
					}

					// TODO: When seeding is implemented this will be necessary
					/*
					if mode.seed == true && peer.am_choking() == false {
						// We entered seed mode and should inform the peer
						log::debug!("[{pid}] Sending Message::Unchoke");
						peer.send(Message::Unchoke).await.unwrap();
					} else if mode.seed == false && peer.am_choking() == true {
						// We exited seed mode and should inform the peer
						log::debug!("[{pid}] Sending Message::Choke");
						peer.send(Message::Choke).await.unwrap();
					}
					*/
				}
			}
		}
	}
}
