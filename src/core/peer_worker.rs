use std::{cmp::Reverse, ops::Deref, sync::Arc, time::Duration};

use tokio::sync::Mutex;

use rand::seq::IteratorRandom;

use tap::tap::Tap;

use crate::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        peer::{Peer, PeerError},
        piece,
        session::{PeerPtr, TorrentPtr},
        util,
    },
    protocol::wire::Message,
};

// TODO: Just nu tänker jag att vi kollar torrent.pieces som är Downloading för att avgöra om vi behöver en till downloader - annars kollar vi om peeren är interested. Vi ändrar state efter det men uppdaterar statet endast var 10:e sekund

use super::{piece::Piece, session::EventSender};

/// An increased block size means an increase in performance, but doesn't work on many clients. 16 KiB is the default.
const BLOCK_SIZE: usize = 16_384;

/// Sever the connection if the peer doesn't send a message within this duration. This amount of time is generally 2 minutes.
const ALIVE_TIMEOUT: Duration = Duration::from_secs_f64(120.0);

/// The maximum amount of simultaneous requests that can be sent to a peer
const MAX_REQUESTS: usize = 5;

/// If Endgame should be enabled
const ENABLE_ENDGAME: bool = true;

enum DownloadError {
    Timeout,
    Choked,
    MessageError,
    PeerError(PeerError),
}

impl From<tokio::time::error::Elapsed> for DownloadError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        DownloadError::Timeout
    }
}

impl From<PeerError> for DownloadError {
    fn from(error: PeerError) -> Self {
        DownloadError::PeerError(error)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum State {
    /// The worker is currently idling
    Idle,
    /// The worker is currently downloading a piece
    Download(usize),
    /// The worker is currently seeding
    Seed(usize),
}

#[derive(Debug)]
pub struct Worker {
    peer: PeerPtr,
    // pub state: Arc<Mutex<State>>,
    pub state_tx: tokio::sync::watch::Sender<State>,
    process: tokio::task::JoinHandle<Result<(), ()>>, // TODO: WorkerError
}

impl Worker {
    pub fn spawn(peer: PeerPtr, torrent: TorrentPtr, tx: EventSender) -> Worker {
        let (state_tx, state_rx) = tokio::sync::watch::channel(State::Idle);
        // let state = Arc::new(Mutex::new(State::Idle));

        let worker_peer = peer.clone();

        let process =
            tokio::spawn(async move { run_worker(worker_peer, torrent, tx, state_rx).await });

        Worker {
            peer,
            // state,
            state_tx,
            process,
        }
    }

    pub fn peer(&self) -> PeerPtr {
        self.peer.clone()
    }

    pub fn set_state(&self, state: State) {
        self.state_tx.send(state).unwrap();
    }

    pub async fn join(self) -> Result<(), ()> {
        self.process.await.expect("Failed to join worker")
    }
}

async fn run_worker(
    peer: PeerPtr,
    torrent: TorrentPtr,
    tx: EventSender,
    mut state_rx: tokio::sync::watch::Receiver<State>,
) -> Result<(), ()> {
    peer.lock()
        .await
        .send(Message::Interested)
        .await
        .expect("Failed to send Message::Interested");

    loop {
        let state: State = *state_rx.borrow().deref();
        match state {
            State::Idle => {
                state_rx.changed().await.unwrap();
                continue;
            }
            State::Download(piece) => {
                // 1. Wait until we are unchoked
                if peer.lock().await.peer_choking() {
                    peer.lock()
                        .await
                        .receive()
                        .await
                        .expect("Failed to receive message (while waiting for unchoke)");
                }

                // 2. Select a piece to download
                // let (piece, piece_length) = {
                //     let lock = torrent.lock().await;

                //     let piece = match select_piece(&lock.pieces, &peer) {
                //         Some(i) => i,
                //         // TODO: If there are pieces with piece::State::Ignore the Peer should be kept alive in case one of them gets enabled
                //         None => break, //todo!("implement a method to wait for pieces to become available")
                //     };

                //     let piece_length = if piece == lock.meta_info.pieces.len() - 1 {
                //         lock.meta_info.last_piece_size
                //     } else {
                //         lock.meta_info.piece_size
                //     };

                //     (piece, piece_length)
                // };

                let piece_length = {
                    let lock = torrent.lock().await;

                    if piece == lock.meta_info.pieces.len() - 1 {
                        lock.meta_info.last_piece_size
                    } else {
                        lock.meta_info.piece_size
                    }
                };

                torrent.lock().await.pieces[piece].state = piece::State::Downloading;

                log::trace!(
                    "Downloading piece {} [{}]",
                    piece,
                    util::hex(peer.lock().await.peer_id())
                );

                // 3. Get the piece
                match get_piece(peer.clone(), piece, piece_length).await {
                    Ok(data) => {
                        log::trace!(
                            "Piece {} downloaded [{}]",
                            piece,
                            util::hex(peer.lock().await.peer_id())
                        );

                        torrent.lock().await.pieces[piece].state = piece::State::Downloaded;
                        tx.send(Event::TorrentEvent(
                            torrent.clone(),
                            TorrentEvent::PieceEvent(piece, PieceEvent::Downloaded(Arc::new(data))),
                        ))
                        .unwrap();
                    }
                    Err(err) => {
                        log::error!("Failed to download piece {}", piece);

                        // TODO: In the future we might want to notify other peer workers that the piece is available
                        torrent.lock().await.pieces[piece].state = piece::State::Pending;
                        // work_queue.lock().await.push_back(work);

                        match err {
                            DownloadError::Timeout => {
                                log::error!("Connection timed out");
                                break;
                            }
                            DownloadError::PeerError(error) => {
                                log::error!("A peer error occurred: {:#?}", error);
                                break;
                            }
                            _ => (),
                        }
                    }
                }
            }
            State::Seed(_piece) => {
                todo!("Seeding not yet implemented")
            }
        }
    }

    log::info!(
        "Severing connection [{}]",
        util::hex(peer.lock().await.peer_id())
    );

    Ok(())
}

// Right now we can't send and receive at the same time, needed for seeding
async fn get_piece(
    peer: Arc<Mutex<Peer>>,
    piece: usize,
    piece_length: usize,
) -> Result<Vec<u8>, DownloadError> {
    let mut peer = peer.lock().await;

    let mut blocks = Vec::new();

    let mut active_requests = 0;

    // TODO: Make this code cleaner. Right now we have two blocks of identical piece receiving code.
    // TODO: Use a tokio::sync::Semaphore - https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html

    for i in (0..piece_length).step_by(BLOCK_SIZE) {
        let block_size = (piece_length - i).min(BLOCK_SIZE);

        log::trace!("Requesting piece {} block {}", piece, i);
        tokio::time::timeout(
            ALIVE_TIMEOUT,
            peer.send(Message::Request(piece as u32, i as u32, block_size as u32)),
        )
        .await??;

        active_requests += 1;

        while active_requests >= MAX_REQUESTS {
            match tokio::time::timeout(ALIVE_TIMEOUT, peer.receive()).await?? {
                Message::Piece(index, begin, block) => {
                    log::trace!(
                        "Received piece {} block {} ({:.1}%)",
                        index,
                        begin,
                        100.0 * (begin as f64 + block.len() as f64) / piece_length as f64
                    );
                    blocks.push((begin, block));
                    active_requests -= 1;
                }
                Message::Choke => {
                    return Err(DownloadError::Choked);
                }
                Message::KeepAlive | Message::Unchoke => (),
                msg => {
                    // TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
                    log::error!("Unexpected message while receiving piece: {:?}", msg);
                    return Err(DownloadError::MessageError);
                }
            }
        }
    }

    while active_requests > 0 {
        match tokio::time::timeout(ALIVE_TIMEOUT, peer.receive()).await?? {
            Message::Piece(index, begin, block) => {
                log::trace!(
                    "Received piece {} block {} ({:.1}%)",
                    index,
                    begin,
                    100.0 * (begin as f64 + block.len() as f64) / piece_length as f64
                );
                blocks.push((begin, block));
                active_requests -= 1;
            }
            Message::Choke => {
                return Err(DownloadError::Choked);
            }
            Message::KeepAlive | Message::Unchoke => (),
            msg => {
                // TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
                log::error!("Unexpected message while receiving piece: {:?}", msg);
                return Err(DownloadError::MessageError);
            }
        }
    }

    // Assemble the blocks into a piece
    log::trace!("Assembling piece {}", piece);
    // blocks.sort_by(|(a, _), (b, _)| a.cmp(&b));
    blocks.sort_by_key(|(offset, _)| *offset);
    let data = blocks
        .into_iter()
        .flat_map(|(_offset, data)| data)
        .collect::<Vec<_>>();

    Ok(data)
}

/// Select a piece to work on
///
/// 1. Download all pieces with Priority::Highest
/// 2. Download all pieces in order of Priority::High, Priority::Normal and Priority::Low
/// 3. Download all pieces with Priority::Lowest
///
/// TODO: Should probably be done more cleanly
fn select_piece(pieces: &[Piece], peer: &Peer) -> Option<usize> {
    let pieces = pieces
        .iter()
        .enumerate()
        .filter(|(i, _)| peer.has_piece(*i)) // TODO: Untested
        .collect::<Vec<_>>()
        .tap_mut(|pieces| pieces.sort_by_key(|(_i, piece)| Reverse(&piece.priority)));

    if let Some(i) = pieces
        .iter()
        .filter(|(_i, piece)| {
            piece.priority == piece::Priority::Highest
                && (piece.state == piece::State::Pending
                    || piece.state == piece::State::Downloading)
        })
        .choose(&mut rand::thread_rng())
        .map(|(i, _piece)| *i)
    {
        Some(i)
    } else if let Some(i) = pieces
        .iter()
        .filter(|(_i, piece)| {
            piece.state == piece::State::Pending && piece.priority >= piece::Priority::Low
        })
        .map(|(i, _piece)| *i)
        .next()
    {
        Some(i)
    } else if !pieces.iter().any(|(_i, piece)| {
        (piece.state == piece::State::Pending || piece.state == piece::State::Downloading)
            && piece.priority > piece::Priority::Lowest
    }) {
        pieces
            .iter()
            .find(|(_i, piece)| {
                piece.state == piece::State::Pending && piece.priority == piece::Priority::Lowest
            })
            .map(|(i, _piece)| *i)
    } else if ENABLE_ENDGAME {
        pieces
            .iter()
            .find(|(_i, piece)| piece.state == piece::State::Downloading)
            .map(|(i, _piece)| *i)
    } else {
        None
    }
}
