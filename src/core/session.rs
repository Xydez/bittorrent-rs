use std::sync::Arc;

use sha1::{Digest, Sha1};
use tokio::sync::{Mutex, Semaphore, RwLock};

use crate::{
    core::{event::Event, peer::Peer, util},
    io::store::Store,
    protocol::{
        metainfo::MetaInfo,
        tracker::Announce,
        wire::{Message, self},
    },
};

use super::{
    bitfield::Bitfield,
    event::{PieceEvent, TorrentEvent},
    worker::{self, Mode},
    piece,
    torrent::Torrent,
    piece_download::PieceDownload,
};

/* Type definitions */

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReceiver = tokio::sync::mpsc::UnboundedReceiver<Event>;
pub type PieceID = u32;
pub type TorrentPtr = Arc<RwLock<Torrent>>;
pub type PeerPtr = Arc<Mutex<Peer>>;
pub type PieceDownloadPtr = Arc<Mutex<PieceDownload>>;

pub trait EventCallback = Fn(&Session, &Event);

/* Application constants */

/// Timeout when attempting to connect to a peer
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs_f64(8.0);

/// Sever the connection if the peer doesn't send a message within this duration. This amount of time is generally 2 minutes.
pub const ALIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs_f64(120.0);

/// Size of a block. 16 KiB is the maximum permitted by the spec.
pub const BLOCK_SIZE: u32 = 16_384;

/// The maximum amount of concurrent requests that can be sent to a peer (normally 5)
pub const BLOCK_CONCURRENT_REQUESTS: usize = 5;

/// The maximum number of active piece verification jobs
pub const VERIFICATION_WORKERS: usize = 4;

pub struct Session<'a> {
    pub peer_id: [u8; 20],
    pub torrents: Vec<TorrentPtr>,
    listeners: Vec<Box<dyn EventCallback + 'a>>,
    // TODO: Separate channel for sending a clone of the Event to the user
    tx: EventSender,
    rx: EventReceiver,
    /// Semaphore to track the number of pieces that can be verified at the same time
    verification_semaphore: Arc<Semaphore>,
}

impl<'a> Session<'a> {
    /// Constructs a new session with the specified peer id
    pub fn new(peer_id: [u8; 20]) -> Session<'a> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        Session {
            peer_id,
            torrents: Vec::new(),
            listeners: Vec::new(),
            tx,
            rx,
            verification_semaphore: Arc::new(Semaphore::new(VERIFICATION_WORKERS)),
        }
    }

    /// Adds an event listener to the session
    pub fn add_listener<F: EventCallback + 'a>(&mut self, listener: F) {
        self.listeners.push(Box::new(listener));
    }

    /// Adds a torrent to the session
    pub fn add(&self, meta_info: MetaInfo, store: Box<dyn Store>) {
        let torrent = Arc::new(RwLock::new(Torrent::new(meta_info, store)));

        self.tx
            .send(Event::TorrentEvent(torrent, TorrentEvent::Added))
            .unwrap();
    }

    /// Starts the event loop of the session
    pub async fn start(&mut self) {
        while let Some(event) = self.rx.recv().await {
            match &event {
                Event::Started => {
                    log::info!("Event::Started");
                }
                Event::Stopped => {
                    log::info!("Event::Stopped");
                }
                Event::TorrentEvent(torrent, event) => match event {
                    TorrentEvent::Added => {
                        // The only source of the torrent added event is from session.add which does not have &mut self
                        self.torrents.push(torrent.clone());

                        log::info!(
                            "TorrentEvent::Added {}",
                            util::hex(&torrent.read().await.meta_info.info_hash)
                        );

                        tokio::spawn(try_announce(
                            self.tx.clone(),
                            self.peer_id,
                            torrent.clone(),
                        ));
                    }
                    TorrentEvent::Announced(response) => {
                        log::info!(
                            "TorrentEvent::Announced {}",
                            util::hex(&torrent.read().await.meta_info.info_hash)
                        );

                        let torrent = torrent.clone();

                        let peer_id = self.peer_id;
                        let info_hash = torrent.read().await.meta_info.info_hash;

                        // Add all peers to the torrent
                        for addr in response.peers_addrs.clone() {
                            let torrent = torrent.clone();
                            let tx = self.tx.clone();

                            // TODO: Use a tokio::task::JoinSet instead
                            tokio::spawn(async move {
                                let mut peer = match tokio::time::timeout(
                                    CONNECT_TIMEOUT,
                                    Peer::connect(
                                        addr,
                                        wire::Handshake {
                                            extensions: [0; 8],
                                            info_hash,
                                            peer_id,
                                        },
                                    ),
                                )
                                .await
                                {
                                    Ok(Ok(peer)) => {
                                        log::info!(
                                            "Connected to peer [{}]",
                                            util::hex(peer.peer_id())
                                        );
                                        peer
                                    }
                                    Ok(Err(error)) => {
                                        log::error!("Failed to connect to peer. {}", util::error_chain(error));
                                        return;
                                    }
                                    Err(_) => {
                                        log::error!(
                                            "Failed to connect to peer. Connection timed out."
                                        );
                                        return;
                                    }
                                };

                                // Create a bitfield of our pieces
                                let bitfield = Bitfield::from_bytes(
                                    &torrent
                                        .read()
                                        .await
                                        .pieces
                                        .chunks(8)
                                        .map(|pieces| {
                                            pieces.iter().enumerate().fold(
                                                0u8,
                                                |acc, (i, piece)| {
                                                    if piece.state == piece::State::Done { // TODO: >= piece::State::Verified
                                                        acc + (1 << (7 - i))
                                                    } else {
                                                        acc
                                                    }
                                                },
                                            )
                                        })
                                        .collect::<Vec<u8>>(),
                                );

                                peer.send(Message::Bitfield(bitfield)).await.unwrap();

                                {
                                    let peer = Arc::new(Mutex::new(peer));
                                    let (mode_tx, mode_rx) = tokio::sync::watch::channel(
                                        Mode {
                                            download: true,
                                            seed: false
                                        }
                                    );

                                    let task = worker::spawn(torrent.clone(), peer.clone(), tx, mode_rx);

                                    torrent.write().await.peers.push(worker::Worker {
                                        mode_tx,
                                        peer,
                                        task 
                                    });
                                }
                            });
                        }
                    }
                    TorrentEvent::PieceEvent(piece, event) => match event {
                        PieceEvent::Block(block) => {
                            let Some(download) = torrent.read().await.downloads.get(piece).cloned() else {
                                // This probably means that the download succeeded but we did not cancel the piece
                                log::warn!("download for piece {} block {} not found, block downloaded in vain", piece, block);
                                continue;
                            };

                            let download_lock = download.lock().await;

                            if let Some(data) = download_lock.data() {
                                // Remove the piece download because it is done
                                {
                                    let mut lock = torrent.write().await;
                                    lock.downloads.remove(piece);
                                    lock.pieces[*piece as usize].state = piece::State::Downloaded;
                                }

                                log::debug!("PieceEvent::Block | piece={piece} | Final block received, emitting TorrentEvent::Downloaded");

                                self.tx.send(Event::TorrentEvent(
                                    torrent.clone(),
                                    TorrentEvent::PieceEvent(
                                        *piece,
                                        PieceEvent::Downloaded(Arc::new(data))
                                    )
                                )).unwrap();
                            }
                        },
                        PieceEvent::Downloaded(data) => {
                            log::debug!("PieceEvent::Downloaded | piece={piece} | Piece downloaded, starting verification worker");

                            // TODO: Spawn a thread that verifies the data (verification workers)
                            let semaphore = self.verification_semaphore.clone();
                            let tx = self.tx.clone();
                            let piece = *piece;
                            let torrent = torrent.clone();
                            let data = data.clone();

                            tokio::spawn(async move {
                                let _permit = semaphore.acquire_owned().await.unwrap();
                                torrent.write().await.pieces[piece as usize].state = piece::State::Verifying;

                                let hash = torrent.read().await.meta_info.pieces[piece as usize];
                                //let intact = verify_piece(&data, &hash);
                                let intact = async_verify(data.clone(), &hash).await;

                                torrent.write().await.pieces[piece as usize].state = if intact {
                                    piece::State::Verified
                                } else {
                                    piece::State::Pending // TODO: If we use Block we also need to reset the block state
                                };

                                if intact {
                                    tx.send(Event::TorrentEvent(
                                        torrent,
                                        TorrentEvent::PieceEvent(piece, PieceEvent::Verified(data)),
                                    ))
                                    .unwrap();
                                }
                            });
                        }
                        PieceEvent::Verified(data) => {
                            log::debug!("PieceEvent::Verified | piece={piece} | Piece verified, writing to store");

                            // Store the piece
                            let tx = self.tx.clone();
                            let data = data.clone();
                            let torrent = torrent.clone();
                            let piece = *piece;

                            let lock = torrent.read().await;
                            //let piece_size = lock.meta_info.piece_size;
                            let store = lock.store.clone();
                            drop(lock);

                            tokio::spawn(async move {
                                tokio::task::spawn_blocking(move || {
                                    store
                                        .blocking_lock()
                                        .set(piece as usize, &data)
                                        .expect("Failed to write to store");
                                }).await.unwrap();

                                torrent.write().await.pieces[piece as usize].state = piece::State::Done;
                                tx.send(Event::TorrentEvent(
                                    torrent,
                                    TorrentEvent::PieceEvent(piece, PieceEvent::Done),
                                ))
                                .unwrap();
                            });
                        } // TODO: (Maybe) push the data onto an io writer queue. How do we pass around the data?
                        PieceEvent::Done => {
                            if torrent.read().await.is_done() {
                                self.tx.send(Event::TorrentEvent(torrent.clone(), TorrentEvent::Done)).unwrap();
                            }
                        }
                    },
                    TorrentEvent::Done => {
                        log::info!("Torrent {} is done.", torrent.read().await.meta_info.name);
                    },
                },
            }

            // Inform the listeners of the event
            for listener in &self.listeners {
                listener(self, &event);
            }

            // Stop the event loop if the session has stopped
            if matches!(event, Event::Stopped) {
                break;
            }
        }
    }

    /// Initiates a shutdown of the session
    pub fn shutdown(&self) {
        self.tx.send(Event::Stopped).unwrap();
        // TODO: We should make this async and join all handlers and stuff
        // TODO: Check if we are even running (if the event loop is running)
    }
}

/// Announces a torrent
async fn try_announce(tx: EventSender, peer_id: [u8; 20], torrent: TorrentPtr) {
    let mut torrent_lock = torrent.write().await;

    let response = {
        let mut i = 0;

        let announce = Announce {
            info_hash: torrent_lock.meta_info.info_hash,
            peer_id,
            ip: None,
            port: 8000,
            uploaded: 0,
            downloaded: 0,
            left: 0,
            event: None, // TODO: Should probably be started on the first announce
        };

        loop {
            const ANNOUNCE_RETRIES: i32 = 5;

            if i < ANNOUNCE_RETRIES {
                log::trace!(
                    "Announcing to tracker \"{}\"",
                    torrent_lock.tracker.announce_url()
                );

                let response = torrent_lock.tracker.announce(&announce).await;

                match response {
                    Ok(response) => break Some(response),
                    Err(error) => log::error!("Failed to announce: {}", error),
                }
            } else {
                break None;
            }

            i += 1;
        }
    };

    // Find peers for the torrent
    // TODO: function in peer_worker

    if let Some(response) = response {
        // for addr in announce.peers_addrs {}
        drop(torrent_lock);
        tx.send(Event::TorrentEvent(
            torrent,
            TorrentEvent::Announced(response),
        ))
        .unwrap();
    } else {
        log::error!(
            "Torrent {} failed to announce",
            util::hex(&torrent_lock.meta_info.info_hash)
        );
    }
}

/// Calculates the size of a piece using the [`MetaInfo`]
pub(crate) fn piece_size(piece: PieceID, meta_info: &MetaInfo) -> usize {
    if piece as usize == meta_info.pieces.len() - 1 {
        meta_info.last_piece_size
    } else {
        meta_info.piece_size
    }
}

/// Computes the Sha1 hash of the piece and compares it to the specified hash, returning whether there is a match
async fn async_verify(piece: Arc<Vec<u8>>, hash: &[u8; 20]) -> bool {
    // Use spawn_blocking because it is a CPU bound task
    hash == &tokio::task::spawn_blocking(move || {
        <[u8; 20]>::try_from(Sha1::digest(&*piece).as_slice()).unwrap()
    }).await.unwrap()
}
