use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        peer::{PeerError},
        session::{EventSender, PeerPtr, PieceID, TorrentPtr, self, BLOCK_CONCURRENT_REQUESTS},
        torrent::PieceDownload, block, util,
    },
    protocol::wire::Message,
};

#[derive(Error, Debug)]
pub enum WorkerError {
    #[error("Request timed out")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("Choked during download")]
    Choked,
    #[error("An error occurred while communicating with the peer")]
    PeerError(#[from] PeerError),
    #[error("Peer is shutting down")]
    Shutdown
}

type Result<T> = std::result::Result<T, WorkerError>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Mode {
    pub download: bool,
    pub seed: bool
}

#[derive(Debug)]
pub struct Worker {
    pub peer: PeerPtr,
    pub mode_tx: tokio::sync::watch::Sender<Mode>,
    pub task: tokio::task::JoinHandle<Result<()>>
}

pub fn spawn(
    torrent: TorrentPtr,
    peer: PeerPtr,
    event_tx: EventSender,
    mut mode_rx: tokio::sync::watch::Receiver<Mode>
) -> tokio::task::JoinHandle<Result<()>> {
    log::trace!("starting worker");

    tokio::spawn(async move {
        let (message_send_tx, mut message_send_rx) = tokio::sync::mpsc::channel::<Message>(128);
        let (message_recv_tx, _) = tokio::sync::broadcast::channel::<Message>(128);

        let picker = torrent.lock().await.picker.clone(); // <- TODO: Redundant clone?
        let mut peer = peer.lock().await;
        let pid = peer.peer_id_short();

        let block_semaphore = Arc::new(tokio::sync::Semaphore::new(BLOCK_CONCURRENT_REQUESTS));

        // The currently active piece download
        let mut current_piece: Option<(PieceID, Arc<Mutex<PieceDownload>>)> = None;

        // True if we need to check whether there are downloadable blocks in the current download
        let mut maybe_blocks = false;

        loop {
            let block_semaphore = block_semaphore.clone();

            tokio::select! {
                // Forward messages from message_send
                message = message_send_rx.recv() => {
                    let message = message.unwrap();

                    log::trace!("[{pid}] forwarding message from block task {}", message);
                    peer.send(message).await.unwrap();
                }
                // Forward messages to message_recv
                message = peer.receive() => {
                    let message = match message {
                        Ok(message) => message,
                        Err(error) => {
                            log::error!("[{pid}] failed to receive message: {}", util::error_chain(&error));
                            return Err(WorkerError::PeerError(error));
                        }
                    };

                    if let Message::Have(_) | Message::Bitfield(_) = message {
                        // If we receive a have/bitfield message it means there is a new piece for us to download. If we didn't have any pieces to download from this peer but we now do it means we should set the current piece to something

                        // has_blocks is true if there are any pending blocks in the current download
                        let has_blocks = if let Some((_, download)) = &current_piece {
                            picker.read().await.select_block(&*download.lock().await).is_some()
                        } else {
                            false
                        };

                        // If current_piece is completed or None we find a new piece
                        if !has_blocks {
                            let mut torrent = torrent.lock().await;

                            let piece = torrent.picker.read().await.select_piece(&torrent, &peer);
                            current_piece = piece.map(|piece| {
                                let piece_size = session::piece_size(piece, &torrent.meta_info);

                                (
                                    piece,
                                    torrent.downloads
                                        .entry(piece)
                                        .or_insert_with(|| Arc::new(Mutex::new(PieceDownload::new(piece_size))))
                                        .clone()
                                )
                            });

                            maybe_blocks = piece.is_some()
                        }
                    }

                    log::trace!("[{pid}] forwarding message to block task {}", message);
                    if message_recv_tx.send(message).is_err() {
                        log::trace!("[{pid}] no message receivers");
                    }
                }
                // If downloading is enabled, we are unchoked and there are blocks available, download them
                permit = block_semaphore.acquire_owned(), if !peer.peer_choking() && mode_rx.borrow().download && maybe_blocks => {
                    // has_blocks is true if there are any pending blocks in the current download
                    let has_blocks = if let Some((_, download)) = &current_piece {
                        picker.read().await.select_block(&*download.lock().await).is_some()
                    } else {
                        false
                    };

                    if !has_blocks {
                        log::trace!("[{pid}] no blocks to download");
                        maybe_blocks = false;
                        continue;
                    }

                    // Pick a block to download using the torrent's picker and set it to downloading
                    let block = {
                        let (_, download) = current_piece.as_ref().unwrap();

                        let mut lock = download.lock().await;

                        let block_i = picker
                            .read()
                            .await
                            .select_block(&*lock)
                            .expect("Failed to pick a block"); // TODO: Continue idling instead of crashhing?

                        lock.blocks[block_i].state = block::State::Downloading;

                        block_i
                    };

                    // TODO: Find a way to catch errors in get_block and return them

                    log::trace!("[{pid}] starting get_block");
                    get_block(
                        pid.clone(),
                        permit.unwrap(),
                        torrent.clone(),
                        current_piece.as_ref().unwrap().clone(),
                        block,
                        message_send_tx.clone(),
                        message_recv_tx.subscribe(),
                        event_tx.clone()
                    );
                }
                mode = mode_rx.changed() => {
                    if mode.is_err() {
                        // The session has shut down and we should exit gracefully
                        break Ok(());
                    }
                }
            }

            // TODO: Break with errors instead of panicking
        }
    })
}

fn get_block(
    pid: String,
    permit: tokio::sync::OwnedSemaphorePermit,
    torrent: TorrentPtr,
    (piece, piece_download): (PieceID, Arc<Mutex<PieceDownload>>),
    block: usize,
    message_tx: tokio::sync::mpsc::Sender<Message>,
    mut message_rx: tokio::sync::broadcast::Receiver<Message>,
    event_tx: EventSender
) -> tokio::task::JoinHandle<Result<Vec<u8>>> {
    tokio::spawn(async move {
        let _permit = permit;

        let (block_begin, block_size) = {
            let block = &mut piece_download.lock().await.blocks[block];
            (block.begin, block.size)
        };

        log::trace!("[{pid}] Requesting piece {} block {}-{}", piece, block_begin, block_begin + block_size);
        message_tx.send(Message::Request(piece, block_begin, block_size)).await.unwrap();

        let result = loop {
            let message = match message_rx.recv().await {
                Ok(message) => message,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    log::trace!("[{pid}] get_block shutting down");
                    break Err(WorkerError::Shutdown);
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    log::error!("[{pid}] Peer lagged {} messages behind", count);

                    // Only setting state because we panic after
                    piece_download.lock().await.blocks[block].state = block::State::Pending;
                    panic!("Peer lagged {} messages behind", count);
                }
            };

            match message {
                Message::Piece(index, begin, block) if index == piece && begin == block_begin && block.len() == block_size as usize => {
                    break Ok(block);
                },
                Message::Choke => {
                    log::warn!("[{pid}] Choked during download");
                    break Err(WorkerError::Choked);
                },
                _ => ()
            }
        };

        {
            let mut lock = piece_download.lock().await;

            let blocks_total = lock.blocks().count();
            let blocks_done = lock.blocks().filter(|block| matches!(block.state, block::State::Done(_))).count();

            lock.blocks[block].state = match result {
                Ok(ref data) => {
                    log::trace!(
                        "[{pid}] Received piece {} block {}-{} ({}/{} blocks)",
                        piece, block_begin, block_begin + block_size,
                        blocks_done, blocks_total
                    );
                    event_tx.send(Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(piece, PieceEvent::Block(block)))).unwrap();
                    block::State::Done(data.to_vec()) // Clone the data
                },
                Err(_) => block::State::Pending
            };
        }

        // What is the point of returning the data if we are sending the event? Do either but not both.
        // Use the same method of returning errors and blocks
        // Since we are setting block::State::Downloading in `spawn`, we should set it to `Done` there too.
        // A good idea would be to send back a `Result` with a channel
        result 
    })
}
