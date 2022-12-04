use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
	core::{
		bitfield::Bitfield,
		block,
		configuration::Configuration,
		event::{
			Event,
			PieceEvent,
			TorrentEvent
		},
		peer::{
			Peer,
			PeerError
		},
		piece,
		piece_download::PieceDownload,
		session::{
			EventSender,
			PeerPtr,
			PieceId,
			TorrentPtr
		},
		torrent::Torrent,
		util
	},
	protocol::wire::message::Message
};

#[derive(Error, Debug)]
pub enum Error {
	#[error("Request timed out")]
	Timeout(#[from] tokio::time::error::Elapsed),
	#[error("An error occurred while communicating with the peer")]
	PeerError(#[from] PeerError)
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Mode {
	pub download: bool,
	pub seed: bool
}

// TODO: Use a smarter solution than PieceIterator which is utter garbage

#[derive(Default)]
struct PieceIterator {
	download: Option<(PieceId, Arc<Mutex<PieceDownload>>)>
}

impl PieceIterator {
	// TODO: Since we only use `&Peer` and `&mut Torrent` in one codepath maybe
	//       it should be a PeerPtr but in order to do that we need to stop
	//       having the peer constantly locked in spawn

	/// Finds a block to download in a PieceDownload
	pub async fn take(
		&mut self,
		config: &Configuration,
		torrent: &mut Torrent,
		peer: &Peer
	) -> Option<(Arc<Mutex<PieceDownload>>, PieceId, usize)> {
		if let Some(v) = self.get(torrent).await {
			Some(v)
		} else {
			let piece = torrent.picker.select_piece(torrent, peer);

			self.download = piece.map(|piece| {
				let piece_size = util::piece_size(piece, &torrent.meta_info);

				torrent.pieces[piece as usize].state = piece::State::Downloading;

				(
					piece,
					torrent
						.downloads
						.entry(piece)
						.or_insert_with(|| {
							Arc::new(Mutex::new(PieceDownload::new(config, piece_size)))
						})
						.clone()
				)
			});

			self.get(torrent).await
		}
	}

	async fn get(&self, torrent: &Torrent) -> Option<(Arc<Mutex<PieceDownload>>, PieceId, usize)> {
		if let Some((piece, ref download)) = self.download {
			if let Some(block) = torrent.picker.select_block(&*download.lock().await) {
				return Some((download.clone(), piece, block));
			}
		}

		None
	}
}

pub async fn run(
	config: Arc<Configuration>,
	torrent: TorrentPtr,
	peer: PeerPtr,
	event_tx: EventSender,
	mut mode_rx: tokio::sync::watch::Receiver<Mode>
) -> Result<()> {
	/* Variables */

	let (message_send_tx, mut message_send_rx) = tokio::sync::mpsc::channel::<Message>(128);
	let (message_recv_tx, _) = tokio::sync::broadcast::channel::<Message>(128);

	let mut peer = peer.lock().await; // TODO: What is the point of having a mutex if the peer is constantly locked?
	let pid = peer.peer_id_short();

	let block_semaphore = Arc::new(tokio::sync::Semaphore::new(
		config.concurrent_block_downloads
	));

	// Keeps track of the current piece download
	let mut picker_iter = PieceIterator::default();
	let mut block_tasks = tokio_util::task::JoinMap::new();

	// True if we need to check whether there are downloadable blocks in the current download
	// False until we receive a bitfield from the peer
	let mut maybe_blocks = false;

	/* Initialize */
	// Create a bitfield of our pieces
	let bitfield = Bitfield::from_bytes(
		&torrent
			.read()
			.await
			.pieces
			.chunks(8)
			.map(|pieces| {
				pieces.iter().enumerate().fold(0u8, |acc, (i, piece)| {
					if piece.state == piece::State::Done {
						// TODO: >= piece::State::Verified
						acc + (1 << (7 - i))
					} else {
						acc
					}
				})
			})
			.collect::<Vec<u8>>()
	);

	log::debug!("[{pid}] Sending bitfield to peer");
	peer.send(Message::Bitfield(bitfield)).await.unwrap();

	// Send interested/unchoke messages on start
	{
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
		let last_message_sent = peer.last_message_sent();

		tokio::select! {
			// Forward messages from message_send
			message = message_send_rx.recv() => {
				let message = message.unwrap();

				log::trace!("[{pid}] Forwarding message from block task {}", message);
				peer.send(message).await?;
			},
			// Forward messages to message_recv
			message = peer.receive() => {
				let message = match message {
					Ok(message) => message,
					Err(error) => {
						log::error!("[{pid}] Failed to receive message: {}", util::error_chain(&error));
						return Err(Error::PeerError(error));
					}
				};

				if let Message::Have(_) | Message::Bitfield(_) = message {
					// If we receive a have/bitfield message it means there is a new piece for us to download. If we didn't have any pieces to download from this peer but we now do it means we should set the current piece to something

					// TODO: Maybe only set to true if we don't already have the piece
					maybe_blocks = true;
				}

				log::trace!("[{pid}] Forwarding message to block task: {}", message);

				if message_recv_tx.send(message).is_err() {
					log::trace!("[{pid}] no message receivers");
				}
			},
			// Keep the peer connection alive
			_ = tokio::time::sleep_until(last_message_sent + config.alive_timeout) => {
				peer.send(Message::KeepAlive).await?;
			}
			// If downloading is enabled, we are unchoked and there are blocks available, download them
			permit = block_semaphore.clone().acquire_owned(), if !peer.peer_choking() && mode_rx.borrow().download && maybe_blocks => {
				log::trace!("[{pid}] permit acquired");

				let Some((download, piece, block)) = picker_iter.take(&config, &mut *torrent.write().await, &peer).await else {
					log::debug!("[{pid}] no blocks to download");
					maybe_blocks = false;
					continue;
				};

				let block_region = {
					let block = &mut download.lock().await.blocks[block];

					block.state = block::State::Downloading;
					(block.begin, block.size)
				};

				log::trace!("[{pid}] starting get_block");
				block_tasks.spawn(
					(piece, block),
					block_worker::get_block(
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
				let download = torrent.read().await.downloads[&piece].clone();

				// TODO: Make sure get_block cannot panic
				match result.unwrap() {
					Ok(data) => {
						{
							let mut lock = download.lock().await;

							lock.blocks[block].state = block::State::Done(data.clone());

							let blocks_total = lock.blocks().count();
							let blocks_done = lock.blocks().filter(|block| matches!(block.state, block::State::Done(_))).count();

							let block_begin = lock.blocks[block].begin;
							let block_size = lock.blocks[block].size;

							log::trace!(
								"[{pid}] Received piece {} block {}-{} ({}/{} blocks)",
								piece, block_begin, block_begin + block_size,
								blocks_done, blocks_total
							);
						}

						event_tx.send(Event::TorrentEvent(torrent.read().await.id(), TorrentEvent::PieceEvent(piece, PieceEvent::Block(block)))).unwrap();
					},
					Err(err) => {
						download.lock().await.blocks[block].state = block::State::Pending;

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
						// TODO: Cancel all instances of get_block
						log::debug!("[{pid}] Sending Message::NotInterested");
						peer.send(Message::NotInterested).await.unwrap();
					}

					// TODO: When seeding is implemented this will be necessary
					/*
					if mode.seed == true && peer.am_choking() == false {
						// We entered seed mode and should inform the peer
						// TODO: Implement a choking algorithm instead of unchoking all peers
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

		// TODO: Break with errors instead of panicking
	}
}

mod block_worker;
