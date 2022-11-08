use std::{sync::Arc};

use sha1::{Digest, Sha1};
use tokio::sync::{Mutex, Semaphore};

use crate::{
    core::{event::Event, peer::Peer, util},
    io::store::Store,
    protocol::{
        metainfo::MetaInfo,
        tracker::Announce,
        wire,
    },
};

use super::{
    bitfield::Bitfield,
    event::{PieceEvent, TorrentEvent},
    peer_worker::{self, Mode},
    piece,
    torrent::Torrent,
};

pub(crate) type TorrentPtr = Arc<Mutex<Torrent>>;
pub(crate) type PeerPtr = Arc<Mutex<Peer>>;

/// Timeout when attempting to connect to a peer
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs_f64(8.0);

/// Sever the connection if the peer doesn't send a message within this duration. This amount of time is generally 2 minutes.
pub const ALIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs_f64(120.0);

/// Size of a block. 16 KiB is the maximum permitted by the spec.
pub const BLOCK_SIZE: u32 = 16_384;

/// The maximum amount of concurrent requests that can be sent to a peer
pub const BLOCK_CONCURRENT_REQUESTS: usize = 5;

// TODO:
// Rewrite `peer_worker.rs`
//  - Find a better way to shut them down, would be nice to try to bind their lifetime to `Session` and implement `Drop`. Try to avoid using smart pointers because they keep the application alive. Weak pointers can be used in the worst case.
//      - `Peer::new(torrent: Torrent<'a>) -> Peer<'a>`
//      - Always send a `bittorrent::wire::Message::Cancel` when the peer worker shuts down
//  - Using the new states we should also create a `verification_worker.rs` that handles verifying pieces.
//  - The current idea is that workers scan the torrent pieces and follow prioritization rules to find work.
//      - Add a flag `end_game` which enables
//      - Add a new `Settings` struct that contains the aforementioned flag.
//          - Maybe also add a builder.

// TODO:
// Just copy over things from the old client, we want almost everything except the work queue which should be replaced by the new peer and verification workers. Also we didn't used to join the old peer worker, which was pretty damn stupid.

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReceiver = tokio::sync::mpsc::UnboundedReceiver<Event>;
pub type PieceID = u32;

pub trait EventCallback = Fn(&Session, &Event);

pub struct Session<'a> {
    pub peer_id: [u8; 20],
    pub torrents: Vec<TorrentPtr>,
    listeners: Vec<Box<dyn EventCallback + 'a>>,
    tx: EventSender, // TODO: Separate channel for sending a clone of the Event to the user
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
            verification_semaphore: Arc::new(Semaphore::new(4)),
        }
    }

    /// Adds an event listener to the session
    pub fn add_listener<F: EventCallback + 'a>(&mut self, listener: F) {
        self.listeners.push(Box::new(listener));
    }

    /// Adds a torrent to the session
    pub fn add(&mut self, meta_info: MetaInfo, store: Box<dyn Store>) {
        let torrent = Arc::new(Mutex::new(Torrent::new(meta_info, store)));

        self.torrents.push(torrent.clone());

        self.tx
            .send(Event::TorrentEvent(torrent, TorrentEvent::Added))
            .expect("Cannot add torrent because channel is closed");
    }

    /// Starts the event loop of the session
    pub async fn start(&mut self) {
        while let Some(event) = self.rx.recv().await {
            match &event {
                Event::TorrentEvent(torrent, event) => match &event {
                    TorrentEvent::Added => {
                        log::info!(
                            "Event::TorrentAdded {}",
                            util::hex(&torrent.lock().await.meta_info.info_hash)
                        );

                        tokio::spawn(try_announce(
                            self.tx.clone(),
                            self.peer_id,
                            torrent.clone(),
                        ));
                    }
                    TorrentEvent::Announced(response) => {
                        let torrent = torrent.clone();

                        let peer_id = self.peer_id;
                        let info_hash = torrent.lock().await.meta_info.info_hash;

                        // 1. Add all peers to the torrent
                        for addr in response.peers_addrs.clone() {
                            let torrent = torrent.clone();

                            let tx = self.tx.clone();

                            // TODO: We want to save the handle and do the PeerWorker thing
                            // TODO: Move all of this into `peer_worker::spawn(...)`
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
                                        log::error!("Failed to connect to peer. {}", error);
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
                                        .lock()
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

                                peer.send(wire::Message::Bitfield(bitfield)).await.unwrap();

                                // let peer = Arc::new(Mutex::new(peer));
                                // torrent.lock().await.peers.push(peer.clone());
                                // peer_worker::run_worker(peer, torrent, tx).await;

                                /*
                                {
                                    let mut lock = torrent.lock().await;
                                    let worker = peer_worker::Worker::spawn(
                                        Arc::new(Mutex::new(peer)),
                                        torrent.clone(),
                                        tx,
                                    );
                                    lock.peers.push(worker);
                                }
                                */

                                {
                                    // TODO:
                                    /*
                                    let peer = Arc::new(Mutex::new(peer));
                                    let (state_tx, state_rx) = tokio::sync::watch::channel(peer_worker::State::Idle);
                                    torrent.lock().await.peers.push(peer_worker::Worker {
                                        peer: peer.clone(),
                                        state_tx
                                    });
                                    peer_worker::run_worker(peer, torrent.clone(), tx, state_rx).await.unwrap();
                                    */

                                    let peer = Arc::new(Mutex::new(peer));
                                    let (mode_tx, mode_rx) = tokio::sync::watch::channel(
                                        Mode {
                                            download: true,
                                            seed: false
                                        }
                                    );

                                    let task = peer_worker::start_worker(torrent.clone(), peer.clone(), tx, mode_rx);

                                    torrent.lock().await.peers.push(peer_worker::Worker {
                                        mode_tx,
                                        peer,
                                        task 
                                    });
                                }
                            });
                        }

                        // (not yet sure about this)
                        // 2. Iterate over all peers and select which ones to download from and which ones to seed to
                        /*
                        {
                            let lock = torrent.lock().await;

                            for worker in &lock.peers {
                                //let piece = Torre§

                                //Self::select_piece(&lock, worker);
                                todo!("Probably just do this in peer_worker instead")
                            }
                        }
                        */
                    }
                    TorrentEvent::PieceEvent(piece, event) => match &event {
                        PieceEvent::Block(_) => (),
                        PieceEvent::Downloaded(data) => {
                            // TODO: Spawn a thread that verifies the data
                            let semaphore = self.verification_semaphore.clone();
                            let tx = self.tx.clone();
                            let piece = *piece;
                            let torrent = torrent.clone();
                            let data = data.clone();

                            tokio::spawn(async move {
                                let _permit = semaphore.acquire_owned().await.unwrap();
                                torrent.lock().await.pieces[piece as usize].state = piece::State::Verifying;

                                let hash = torrent.lock().await.meta_info.pieces[piece as usize];
                                let intact = verify_piece(&data, &hash);

                                torrent.lock().await.pieces[piece as usize].state = if intact {
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
                            // Store the piece
                            let tx = self.tx.clone();
                            let data = data.clone();
                            let torrent = torrent.clone();
                            let piece = *piece;

                            let lock = torrent.lock().await;
                            let piece_size = lock.meta_info.piece_size;
                            let store = lock.store.clone();
                            drop(lock);

                            tokio::spawn(async move {
                                store
                                    .lock()
                                    .await
                                    .set(piece as usize * piece_size, &data)
                                    .expect("Failed to write to store");

                                torrent.lock().await.pieces[piece as usize].state = piece::State::Done;
                                tx.send(Event::TorrentEvent(
                                    torrent,
                                    TorrentEvent::PieceEvent(piece, PieceEvent::Done),
                                ))
                                .unwrap();
                            });
                        } // TODO: (Maybe) push the data onto an io writer queue. How do we pass around the data?
                        PieceEvent::Done => (),
                    },
                    TorrentEvent::Done => todo!("Implement TorrentEvent::Done"),
                },
                Event::Shutdown => {
                    log::info!("Event::Shutdown");
                    break;
                }
            }

            for listener in &self.listeners {
                listener(self, &event);
            }
        }
    }

    /// Stops the session
    pub fn shutdown(&self) {
        self.tx.send(Event::Shutdown).unwrap();
        // TODO: We should make this async and join all handlers and stuff
        // TODO: Check if we are even running (if the event loop is running)
        // TODO: `&self` should maybe be `&mut self`?
    }

    
}

/// Announces a torrent
async fn try_announce(tx: EventSender, peer_id: [u8; 20], torrent: TorrentPtr) {
    let mut torrent_lock = torrent.lock().await;

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
fn verify_piece(piece: &[u8], hash: &[u8; 20]) -> bool {
    hash == &<[u8; 20]>::try_from(Sha1::digest(piece).as_slice()).unwrap()
}