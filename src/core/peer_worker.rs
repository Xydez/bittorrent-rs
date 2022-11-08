use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        peer::{PeerError},
        piece::{PieceDownload, BlockState},
        session::{PeerPtr, TorrentPtr, self, BLOCK_CONCURRENT_REQUESTS},
        algorithm,
    },
    protocol::wire::Message,
};

// TODO: Just nu tänker jag att vi kollar torrent.pieces som är Downloading för att avgöra om vi behöver en till downloader - annars kollar vi om peeren är interested. Vi ändrar state efter det men uppdaterar statet endast var 10:e sekund

use super::session::EventSender;

#[derive(Debug)]
pub enum WorkerError {
    Timeout,
    Choked,
    MessageError,
    PeerError(PeerError),
    /// Use with error types that I'm too lazy to have written yet
    WIP
}

impl From<tokio::time::error::Elapsed> for WorkerError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        WorkerError::Timeout
    }
}

impl From<PeerError> for WorkerError {
    fn from(error: PeerError) -> Self {
        WorkerError::PeerError(error)
    }
}

type Result<T> = std::result::Result<T, WorkerError>;

// TODO: Rename to Mode or remove?
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum State {
    /// The worker is currently idling
    Idle,
    /// The worker is currently downloading a piece
    Download,
    /// The worker is currently seeding
    Seed,
}

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

pub fn start_worker(
    torrent: TorrentPtr,
    peer: PeerPtr,
    event_tx: EventSender,
    mut mode_rx: tokio::sync::watch::Receiver<Mode>
) -> tokio::task::JoinHandle<Result<()>> {
    log::trace!("starting worker");

    tokio::spawn(async move {
        let (message_send_tx, mut message_send_rx) = tokio::sync::mpsc::channel::<Message>(16);
        let (message_recv_tx, _) = tokio::sync::broadcast::channel::<Message>(16);
        // TODO: We are dropping `message_recv_rx`, will the channel still work?

        let mut peer = peer.lock().await;
        let pid = peer.peer_id_short();

        let block_semaphore = Arc::new(tokio::sync::Semaphore::new(BLOCK_CONCURRENT_REQUESTS));
        let mut piece_download: Option<Arc<Mutex<PieceDownload>>> = None;
        let mut maybe_pending = false;

        loop {
            // TODO: Is it really efficient to clone the semaphore every iteration?
            // ^ Re: Probably, because the Arc is stack allocated
            let block_semaphore = block_semaphore.clone();

            log::trace!("[{pid}] loop iteration");

            tokio::select! {
                // Forward messages from message_send
                message = message_send_rx.recv() => {
                    log::trace!("[{pid}] forwarding message from block task {:?}", message);
                    peer.send(message.unwrap()).await.unwrap();
                }
                // Forward messages to message_recv
                message = peer.receive() => {
                    let message = message.unwrap();

                    if let Message::Have(_) | Message::Bitfield(_) = message {
                        let has_pending = if let Some(piece_download) = &piece_download {
                            log::trace!("[{pid}] locking piece_download");
                            piece_download.lock().await.has_pending()
                        } else {
                            false
                        };

                        // If current_piece is completed or None we find a new piece
                        if !has_pending {
                            log::trace!("[{pid}] locking torrent");
                            let mut torrent = torrent.lock().await;

                            let possible_pieces = torrent.pieces
                                .iter()
                                .enumerate()
                                .map(|(i, p)| (i as u32, p))
                                .filter(|(i, _)| peer.has_piece(*i))
                                .collect::<Vec<_>>();

                            log::trace!("[{pid}] selecting piece");
                            let piece = algorithm::select_piece(&possible_pieces, torrent.is_endgame()).unwrap();
                            let piece_size = session::piece_size(piece, &torrent.meta_info);

                            piece_download = Some(
                                torrent.downloads
                                    .entry(piece)
                                    .or_insert_with(|| Arc::new(Mutex::new(PieceDownload::new(piece, piece_size))))
                                    .clone()
                            );

                            maybe_pending = true;
                        }
                    }

                    log::trace!("[{pid}] forwarding message to block task {:?}", message);
                    if message_recv_tx.send(message).is_err() {
                        log::trace!("[{pid}] no message receivers");
                    }
                }
                // If downloading is enabled, download blocks
                permit = block_semaphore.acquire_owned(), if mode_rx.borrow().download && maybe_pending => {
                    log::trace!("[{pid}] permit acquired");
                    let has_pending = if let Some(piece_download) = &piece_download {
                        log::trace!("[{pid}] locking piece_download");
                        piece_download.lock().await.has_pending()
                    } else {
                        false
                    };

                    if !has_pending {
                        log::trace!("[{pid}] no pending blocks");
                        maybe_pending = false;
                        continue;
                    }

                    let block = {
                        log::trace!("[{pid}] locking piece_download");
                        let piece_download = piece_download.as_ref().unwrap().lock().await;
                        piece_download.blocks.iter()
                            .enumerate()
                            .find(|(_, block)| block.state == BlockState::Pending) // TODO: Utility function to do this
                            .map(|(i, _)| i)
                            .unwrap()
                    };

                    log::trace!("[{pid}] starting get_block");
                    get_block(
                        pid.clone(),
                        permit.unwrap(),
                        torrent.clone(),
                        piece_download.as_ref().unwrap().clone(),
                        block,
                        message_send_tx.clone(),
                        message_recv_tx.subscribe(),
                        event_tx.clone()
                    );
                }
                mode = mode_rx.changed() => {
                    if mode.is_err() {
                        // The session has shut down and we should exit gracefully
                        break;
                    }
                }
            }

            // TODO: Break out of the loop somehow (when an error occurs or )
        }

        todo!("Maybe return eventual errors?");
    })
}

// TODO: Return Future instead of JoinHandle?

fn get_block(
    pid: String,
    permit: tokio::sync::OwnedSemaphorePermit,
    torrent: TorrentPtr,
    piece_download: Arc<Mutex<PieceDownload>>,
    block: usize,
    message_tx: tokio::sync::mpsc::Sender<Message>,
    mut message_rx: tokio::sync::broadcast::Receiver<Message>,
    event_tx: EventSender
) -> tokio::task::JoinHandle<Result<Vec<u8>>> {
    tokio::spawn(async move {
        let _permit = permit;
        //let (block_begin, block_size) = block;

        let piece = piece_download.lock().await.piece;

        let (block_begin, block_size) = {
            let mut lock = piece_download.lock().await;
            let block = &mut lock.blocks[block];
            block.state = BlockState::Downloading;
            (block.begin, block.size)
        };

        log::trace!("[{pid}] Requesting piece {} block {}", piece, block_begin);
        message_tx.send(Message::Request(piece, block_begin, block_size)).await.unwrap();

        let result = loop {
            let message = match message_rx.recv().await {
                Ok(message) => message,
                Err(error) => {
                    log::error!("[{pid}] Failed to receive message: {}", error);
                    break Err(WorkerError::WIP);
                }
            };

            if let Message::Piece(index, begin, block) = message {
                if index == piece && begin == block_begin && block.len() == block_size as usize {
                    log::trace!("[{pid}] Received piece {} block {}", index, begin);
                    break Ok(block);
                }
            }
        };

        piece_download.lock().await.blocks[block].state = match result {
            Ok(ref data) => {
                event_tx.send(Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(piece, PieceEvent::Block(block)))).unwrap();
                BlockState::Done(data.to_vec())
            },
            Err(_) => BlockState::Pending
        };

        result
    })
}
