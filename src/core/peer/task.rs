use std::sync::Arc;

use log::{debug, error, info, trace, warn};
use tokio::sync::Mutex;

use super::Peer;
use crate::{
	core::{
		algorithm,
		configuration::Configuration,
		event::{PeerEvent, Sender},
		peer::{
			block_worker::{self, get_block},
			Error, Result
		},
		piece::PieceId,
		piece_download::{BlockId, PieceDownload},
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

/*
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
	let Some(piece) = algorithm::select_piece(torrent, peer, config).await else {
		return None;
	};

	let piece_size = util::piece_size(piece, &torrent.torrent.meta_info);

	let downloads_len = torrent.state().downloads.len();

	// 3. Get the PieceDownload
	let download =
		torrent
			.state_mut()
			.downloads
			.entry(piece)
			.or_insert_with(|| {
				debug!("[{pid}] Creating piece download (piece={piece}, downloads_len={downloads_len})");

				Arc::new(Mutex::new(PieceDownload::new(config, piece_size)))
			})
			.clone();

	current_download.replace((piece, download.clone()));

	Some((piece, download))
}
*/

#[derive(Default)]
struct PieceIterator {
	current_piece: Option<(PieceId, Arc<Mutex<PieceDownload>>)>
}

impl PieceIterator {
	pub async fn next<'a>(
		&'a mut self,
		torrent: &'a mut TorrentLock<'_>,
		peer: &Peer,
		pid: &str,
		config: &Configuration
	) -> Option<(Arc<Mutex<PieceDownload>>, PieceId, BlockId)> {
		if let Some((piece, ref download)) = self.current_piece {
			// TODO: Try with end-game
			if let Some(block) = algorithm::select_block(&*download.lock().await, peer.id()) {
				return Some((download.clone(), piece, block));
			}
		}

		// 2. If we can't continue on the current download, select a new piece
		let piece = algorithm::select_piece(torrent, peer, config)?;

		let piece_size = util::piece_size(piece, &torrent.torrent.meta_info);

		let downloads_len = torrent.state().downloads.len();

		if torrent.state().downloads.contains_key(&piece) {
			info!("[{pid}] Assisting with existing download (piece={piece}, downloads_len={downloads_len})");
		}

		// 3. Get the PieceDownload
		let download = torrent
			.state_mut()
			.downloads
			.entry(piece)
			.or_insert_with(|| {
				debug!("[{pid}] Creating piece download (piece={piece}, downloads_len={downloads_len})");

				Arc::new(Mutex::new(PieceDownload::new(config, piece_size)))
			});

		self.current_piece = Some((piece, download.clone()));

		if let Some(block) = algorithm::select_block(&*download.lock().await, peer.id()) {
			return Some((download.clone(), piece, block));
		}

		None
	}
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
	let mut block_tasks = tokio_util::task::JoinMap::new();

	// True if there are currently no blocks that can be downloaded
	let mut out_of_blocks = true;

	// True if no messages have been received from the peer yet
	let mut first_message = true;

	//let mut current_download = None;
	let mut piece_iter = PieceIterator::default();

	// Create a bitfield of our pieces
	let bitfield = torrent.lock().await.bitfield();

	{
		let mut peer = peer.lock().await;

		// Send bitfield on start
		debug!("[{pid}] Sending bitfield to peer");
		peer.send(Message::Bitfield(bitfield.as_bytes().to_vec()))
			.await
			.unwrap();

		// Send interested/unchoke messages on start
		let mode = *mode_rx.borrow_and_update();

		if mode.download {
			debug!("[{pid}] Sending interested message to peer");
			peer.send(Message::Interested).await.unwrap();
		}

		/*
		if mode.seed {
			debug!("[{pid}] Sending Message::Unchoke");
			peer.send(Message::Unchoke).await.unwrap();
		}
		*/
	}

	loop {
		let mut peer = peer.lock().await;
		let last_message_sent = peer.last_message_sent();
		let last_message_received = peer.last_message_received();

		tokio::select! {
			// Forward messages from message_send
			message = message_send_rx.recv() => {
				let message = message.unwrap();

				trace!("[{pid}] SEND: {}", message);
				peer.send(message).await?;
			},
			// Forward messages to message_recv
			message = peer.receive() => {
				let message = match message {
					Ok(message) => message,
					Err(error) => {
						error!("[{pid}] Failed to receive message: {}", util::error_chain(&error));
						return Err(error);
					}
				};

				if !first_message && matches!(&message, Message::Bitfield(_)) {
					error!("Invalid bitfield");
					return Err(Error::IllegalMessage(message));
				}

				if block_tasks.is_empty() && matches!(message, Message::Choke) {
					warn!("[{pid}] Choked during download. Terminating {} block tasks.", block_tasks.len());
				}

				if let Message::Have(_) | Message::Bitfield(_) = message {
					// If we receive a have/bitfield message it means there is a new piece for us to download.
					// TODO: Compare against our own bitfield to see if there are pieces to download
					out_of_blocks = false;
				}

				trace!("[{pid}] RECV: {}", message);

				if message_recv_tx.send(message).is_err() {
					//trace!("[{pid}] No message receivers");
				}

				first_message = false;
			},
			// Keep the peer connection alive
			_ = tokio::time::sleep_until(last_message_sent + config.alive_timeout) => {
				peer.send(Message::KeepAlive).await?;
			}
			// Terminate dead connections
			_ = tokio::time::sleep_until(last_message_received + config.alive_timeout) => {
				break Err(Error::Timeout);
			}
			// If downloading is enabled, we are unchoked and there are blocks available, download them
			permit = block_semaphore.clone().acquire_owned(), if !peer.peer_choking() && mode_rx.borrow().download && !out_of_blocks => {
				/*
				let Some((piece, download)) = get_download(&mut torrent.lock().await, &peer, &pid, &config, &mut current_download).await else {
					debug!("[{pid}] No pieces to download");
					out_of_blocks = true;
					continue;
				};

				let mut download = download.lock().await;

				let Some(block) = algorithm::select_block(&download, peer.id(), false) else {
					debug!("[{pid}] No blocks to download");
					out_of_blocks = true;
					continue;
				};
				*/

				let mut torrent_lock = torrent.lock().await;
				let Some((download, piece, block)) = piece_iter.next(&mut torrent_lock, &peer, &pid, &config).await else {
					debug!("[{pid}] Nothing to download");
					out_of_blocks = true;
					continue;
				};

				let mut download_lock = download.lock().await;

				download_lock.block_downloads.push_back((block, peer.id()));

				let (block_begin, block_size) = (download_lock.blocks[block].begin, download_lock.blocks[block].size);

				trace!("[{pid}] Starting block worker for block {}", util::fmt_block(piece, block));
				block_tasks.spawn(
					(piece, block),
					get_block(
						pid.clone(),
						permit.unwrap(),
						piece,
						(block_begin, block_size),
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
					warn!("[{pid}] Received duplicate block {} (on nonexistent download)", util::fmt_block(piece, block));
					continue;
				};

				let mut download_lock = download.lock().await;

				if let Some(i) = download_lock.block_downloads
					.iter()
					.position(|(block_id, worker_id)| *block_id == block && *worker_id == peer.id())
				{
					download_lock.block_downloads.swap_remove_back(i);
				}

				match result.unwrap() {
					Ok(data) => {
						if download_lock.blocks[block].data.is_none() {
							download_lock.blocks[block].data = Some(data.into());

							let blocks_total = download_lock.blocks().count();
							let blocks_done = download_lock.blocks().filter(|block| block.data.is_some()).count();

							trace!(
								"[{pid}] Received block {} ({}/{} blocks)",
								util::fmt_block(piece, block),
								blocks_done, blocks_total
							);

							event_tx.send((peer.id(), PeerEvent::BlockReceived(piece, block))).unwrap();
						} else {
							warn!("[{pid}] Received duplicate block {}", util::fmt_block(piece, block));
						}
					},
					Err(block_worker::Error::Choked) => (),
					Err(err) => {
						error!("[{pid}] get_blocked errored: {}", util::error_chain(err));
					}
				}
			},
			// Respond to changes in the worker's mode
			result = mode_rx.changed() => {
				if result.is_err() {
					// The session has shut down and we should exit gracefully
					debug!("[{pid}] Worker channel closed, exiting...");
					break Ok(());
				} else {
					let mode = *mode_rx.borrow();

					if !mode.download && peer.am_interested() {
						// We entered download mode and should inform the peer
						debug!("[{pid}] Sending Message::Interested");
						peer.send(Message::Interested).await.unwrap();
					} else if mode.download && !peer.am_interested() {
						// We exited download mode and should inform the peer
						debug!("[{pid}] Sending Message::NotInterested");
						peer.send(Message::NotInterested).await.unwrap();

						// TODO: Send Message::Cancel, either here or when calling join_next
						block_tasks.shutdown().await;
					}

					// TODO: When seeding is implemented this will be necessary
					/*
					if mode.seed == true && peer.am_choking() == false {
						// We entered seed mode and should inform the peer
						debug!("[{pid}] Sending Message::Unchoke");
						peer.send(Message::Unchoke).await.unwrap();
					} else if mode.seed == false && peer.am_choking() == true {
						// We exited seed mode and should inform the peer
						debug!("[{pid}] Sending Message::Choke");
						peer.send(Message::Choke).await.unwrap();
					}
					*/
				}
			}
		}
	}
}
