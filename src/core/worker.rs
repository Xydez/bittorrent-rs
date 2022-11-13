use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        peer::{PeerError},
        session::{EventSender, PeerPtr, PieceID, TorrentPtr, self, BLOCK_CONCURRENT_REQUESTS},
        piece_download::PieceDownload, block, util,
    },
    protocol::wire::Message,
};

use super::{torrent::Torrent, peer::Peer};

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

#[derive(Default)]
struct PieceIterator {
    download: Option<(PieceID, Arc<Mutex<PieceDownload>>)>
}

impl PieceIterator {
    // TODO: Since we only use `&Peer` and `&mut Torrent` in one codepath maybe
    //       it should be a PeerPtr but in order to do that we need to stop
    //       having the peer constantly locked in spawn
    pub async fn take(&mut self, torrent: &mut Torrent, peer: &Peer) -> Option<(Arc<Mutex<PieceDownload>>, PieceID, usize)> {
        if let Some(v) = self.get(torrent).await {
            Some(v)
        } else {
            let piece = torrent.picker.select_piece(torrent, peer);
            self.download = piece.map(|piece| {
                let piece_size = session::piece_size(piece, &torrent.meta_info);

                (
                    piece,
                    torrent.downloads
                        .entry(piece)
                        .or_insert_with(|| Arc::new(Mutex::new(PieceDownload::new(piece_size))))
                        .clone()
                )
            });

            self.get(torrent).await
        }
    }

    async fn get(&self, torrent: &Torrent) -> Option<(Arc<Mutex<PieceDownload>>, PieceID, usize)> {
        if let Some((piece, ref download)) = self.download {
            if let Some(block) = torrent.picker.select_block(&*download.lock().await) {
                return Some((download.clone(), piece, block));
            }
        }

        None
    }
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

        //let picker = torrent.read().await.picker.clone(); // <- TODO: Redundant clone?
        let mut peer = peer.lock().await; // TODO: What is the point of having a mutex if the peer is constantly locked?
        let pid = peer.peer_id_short();

        let block_semaphore = Arc::new(tokio::sync::Semaphore::new(BLOCK_CONCURRENT_REQUESTS));

        // Keeps track of the current piece download
        let mut picker_iter = PieceIterator::default();
        let mut get_block_tasks = tokio::task::JoinSet::new();

        // True if we need to check whether there are downloadable blocks in the current download
        // False until we receive a bitfield from the peer
        let mut maybe_blocks = false;

        // Send interested/unchoke messages on start
        {
            let mode = *mode_rx.borrow_and_update();

            if mode.download {
                log::trace!("[{pid}] Sending Message::Interested");
                peer.send(Message::Interested).await.unwrap();
            }

            /*
            if mode.seed {
                log::trace!("[{pid}] Sending Message::Unchoke");
                peer.send(Message::Unchoke).await.unwrap();
            }
            */
        }

        loop {
            let block_semaphore = block_semaphore.clone();

            tokio::select! {
                // Forward messages from message_send
                message = message_send_rx.recv() => {
                    let message = message.unwrap();

                    log::trace!("[{pid}] forwarding message from block task {}", message);
                    peer.send(message).await.unwrap();
                },
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

                        // TODO: Maybe only set to true if we don't already have the piece
                        maybe_blocks = true;
                    }

                    log::trace!("[{pid}] forwarding message to block task {}", message);
                    if message_recv_tx.send(message).is_err() {
                        log::trace!("[{pid}] no message receivers");
                    }
                },
                // If downloading is enabled, we are unchoked and there are blocks available, download them
                permit = block_semaphore.acquire_owned(), if !peer.peer_choking() && mode_rx.borrow().download && maybe_blocks => {
                    let begin = std::time::Instant::now();
                    let Some((download, piece, block)) = picker_iter.take(&mut *torrent.write().await, &peer).await else {
                        log::trace!("[{pid}] no blocks to download");
                        maybe_blocks = false;
                        continue;
                    };

                    download.lock().await.blocks[block].state = block::State::Downloading;
                    //log::trace!("[{pid}] found block in {:.1} \u{03bc}s", (std::time::Instant::now() - begin).as_micros());

                    log::trace!("[{pid}] starting get_block");
                    get_block_tasks.spawn(
                        get_block(
                            pid.clone(),
                            permit.unwrap(),
                            piece,
                            download.clone(),
                            block,
                            message_send_tx.clone(),
                            message_recv_tx.subscribe(),
                        )
                    );
                },
                // Receive the data from completed get_block tasks
                Some(result) = get_block_tasks.join_next(), if !get_block_tasks.is_empty() => match result {
                    Err(err) => panic!("get_block panicked: {}", err),
                    Ok((piece, block, result)) => {
                        let piece_download = torrent.read().await.downloads[&piece].clone();
                        let mut lock = piece_download.lock().await;

                        match result {
                            Ok(data) => {
                                lock.blocks[block].state = block::State::Done(data.clone());
                                event_tx.send(Event::TorrentEvent(torrent.clone(), TorrentEvent::PieceEvent(piece, PieceEvent::Block(block)))).unwrap();

                                let blocks_total = lock.blocks().count();
                                let blocks_done = lock.blocks().filter(|block| matches!(block.state, block::State::Done(_))).count();

                                let block_begin = lock.blocks[block].begin;
                                let block_size = lock.blocks[block].size;

                                log::trace!(
                                    "[{pid}] Received piece {} block {}-{} ({}/{} blocks)",
                                    piece, block_begin, block_begin + block_size,
                                    blocks_done, blocks_total
                                );
                            },
                            Err(err) => {
                                lock.blocks[block].state = block::State::Pending;
                                log::error!("[{pid}] get_blocked errored: {}", err);
                            }
                        }
                    }
                },
                // Respond to changes in the worker's mode
                result = mode_rx.changed() => {
                    if result.is_err() {
                        // The session has shut down and we should exit gracefully
                        log::trace!("[{pid}] Worker channel closed, exiting...");
                        break Ok(());
                    } else {
                        let mode = *mode_rx.borrow();

                        if !mode.download && peer.am_interested() {
                            // We entered download mode and should inform the peer
                            log::trace!("[{pid}] Sending Message::Interested");
                            peer.send(Message::Interested).await.unwrap();
                        } else if mode.download && !peer.am_interested() {
                            // We exited download mode and should inform the peer
                            // TODO: Cancel all instances of get_block
                            log::trace!("[{pid}] Sending Message::NotInterested");
                            peer.send(Message::NotInterested).await.unwrap();
                        }

                        // TODO: When seeding is implemented this will be necessary
                        /*
                        if mode.seed == true && peer.am_choking() == false {
                            // We entered seed mode and should inform the peer
                            // TODO: Implement a choking algorithm instead of unchoking all peers
                            peer.send(Message::Unchoke).await.unwrap();
                        } else if mode.seed == false && peer.am_choking() == true {
                            // We exited seed mode and should inform the peer
                            peer.send(Message::Choke).await.unwrap();
                        }
                        */
                    }
                }
            }

            // TODO: Break with errors instead of panicking
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn get_block(
    pid: String,
    _permit: tokio::sync::OwnedSemaphorePermit,
    piece: PieceID,
    piece_download: Arc<Mutex<PieceDownload>>,
    block: usize,
    message_tx: tokio::sync::mpsc::Sender<Message>,
    mut message_rx: tokio::sync::broadcast::Receiver<Message>,
) -> (PieceID, usize, Result<Vec<u8>>) {
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
                break (piece, block, Err(WorkerError::Shutdown));
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                log::error!("[{pid}] Peer lagged {} messages behind", count);

                // Only setting state because we panic after
                piece_download.lock().await.blocks[block].state = block::State::Pending;
                panic!("Peer lagged {} messages behind", count);
            }
        };

        match message {
            Message::Piece(index, begin, data) if index == piece && begin == block_begin && data.len() == block_size as usize => {
                break (piece, block, Ok(data));
            },
            Message::Choke => {
                log::warn!("[{pid}] Choked during download");
                break (piece, block, Err(WorkerError::Choked));
            },
            _ => ()
        }
    };

    {
        let mut lock = piece_download.lock().await;

        lock.blocks[block].state = match result.2 {
            Ok(ref data) => block::State::Done(data.clone()),
            Err(_) => block::State::Pending
        };

        /*
        if result.2.is_ok() {
            let blocks_total = lock.blocks().count();
            let blocks_done = lock.blocks().filter(|block| matches!(block.state, block::State::Done(_))).count();

            log::trace!(
                "[{pid}] Received piece {} block {}-{} ({}/{} blocks)",
                piece, block_begin, block_begin + block_size,
                blocks_done, blocks_total
            );

            //event_tx.send(Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(piece, PieceEvent::Block(block)))).unwrap();
        }
        */

        /*
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
        */
    }

    // What is the point of returning the data if we are sending the event? Do either but not both.
    // Use the same method of returning errors and blocks
    // Since we are setting block::State::Downloading in `spawn`, we should set it to `Done` there too.
    // A good idea would be to send back a `Result` with a channel
    result
}
