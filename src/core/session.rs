use std::sync::Arc;

use rand::{Rng, seq::SliceRandom};
use sha1::{Digest, Sha1};
use tap::Tap;
use tokio::sync::{Mutex, Semaphore};

use crate::{
    core::{event::Event, peer::Peer, util},
    io::store::Store,
    protocol::{
        metainfo::MetaInfo,
        tracker::{Announce, Tracker},
        wire,
    },
};

use super::{
    bitfield::Bitfield,
    event::{PieceEvent, TorrentEvent},
    peer_worker,
    piece::{self, Piece},
};

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs_f64(10.0);

pub(crate) type TorrentPtr = Arc<Mutex<Torrent>>;
pub(crate) type PeerPtr = Arc<Mutex<Peer>>;

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

#[derive(Debug)]
pub struct Torrent {
    // TODO: Implement multiple trackers
    pub meta_info: MetaInfo,
    pub peers: Vec<peer_worker::Worker>, // TODO: PeerWorker // PeerPtr
    pub pieces: Vec<Piece>,              // TODO: Should probably be Vec<Mutex<Piece>> instead
    pub store: Arc<Mutex<Box<dyn Store>>>,
    pub tracker: Tracker,
}

impl Torrent {
    /// Calculate the number of completed pieces (piece.state >= State::Verified)
    pub fn completed_pieces(&self) -> usize {
        self.pieces
            .iter()
            .filter(|piece| piece.state >= piece::State::Verified)
            .count()
    }
}

// TODO:
// Just copy over things from the old client, we want almost everything except the work queue which should be replaced by the new peer and verification workers. Also we didn't used to join the old peer worker, which was pretty damn stupid.

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReceiver = tokio::sync::mpsc::UnboundedReceiver<Event>;

pub trait EventCallback = Fn(&Session, &Event);

fn verify_piece(piece: &[u8], hash: &[u8; 20]) -> bool {
    hash == &<[u8; 20]>::try_from(Sha1::digest(piece).as_slice()).unwrap()
}

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

    pub fn add_listener<F: EventCallback + 'a>(&mut self, listener: F) {
        self.listeners.push(Box::new(listener));
    }

    pub fn add(&mut self, meta_info: MetaInfo, store: Box<dyn Store>) {
        let pieces = vec![
            Piece {
                priority: piece::Priority::Normal,
                state: piece::State::Pending,
                availability: 0
            };
            meta_info.pieces.len()
        ];

        let tracker = Tracker::new(&meta_info.announce);

        let torrent = Arc::new(Mutex::new(Torrent {
            meta_info,
            peers: Vec::new(),
            pieces,
            store: Arc::new(Mutex::new(store)),
            tracker,
        }));

        self.torrents.push(torrent.clone());

        self.tx
            .send(Event::TorrentEvent(torrent, TorrentEvent::Added))
            .expect("Cannot add torrent because channel is closed");
    }

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

    pub async fn start(&mut self) {
        while let Some(event) = self.rx.recv().await {
            match &event {
                Event::TorrentEvent(torrent, event) => match &event {
                    TorrentEvent::Added => {
                        log::info!(
                            "Event::TorrentAdded {}",
                            util::hex(&torrent.lock().await.meta_info.info_hash)
                        );

                        tokio::spawn(Session::try_announce(
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
                                    let peer = Arc::new(Mutex::new(peer));
                                    let (state_tx, state_rx) = tokio::sync::watch::channel(peer_worker::State::Idle);
                                    torrent.lock().await.peers.push(peer_worker::Worker {
                                        peer: peer.clone(),
                                        state_tx
                                    });
                                    peer_worker::run_worker(peer, torrent.clone(), tx, state_rx).await.unwrap();
                                }
                            });
                        }

                        // (not yet sure about this)
                        // 2. Iterate over all peers and select which ones to download from and which ones to seed to
                        {
                            let lock = torrent.lock().await;

                            for worker in &lock.peers {
                                Self::select_piece(&lock, worker);
                            }
                        }
                    }
                    TorrentEvent::PieceEvent(piece, event) => match &event {
                        PieceEvent::Downloaded(data) => {
                            // TODO: Spawn a thread that verifies the data
                            let semaphore = self.verification_semaphore.clone();
                            let tx = self.tx.clone();
                            let piece = *piece;
                            let torrent = torrent.clone();
                            let data = data.clone();

                            tokio::spawn(async move {
                                let _permit = semaphore.acquire_owned().await.unwrap();
                                torrent.lock().await.pieces[piece].state = piece::State::Verifying;

                                let hash = torrent.lock().await.meta_info.pieces[piece];
                                let intact = verify_piece(&data, &hash);

                                torrent.lock().await.pieces[piece].state = if intact {
                                    piece::State::Verified
                                } else {
                                    piece::State::Pending
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
                                    .set(piece * piece_size, &data)
                                    .expect("Failed to write to store");

                                torrent.lock().await.pieces[piece].state = piece::State::Done;
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

    pub fn shutdown(&self) {
        self.tx.send(Event::Shutdown).unwrap();
        // TODO: We should make this async and join all handlers and stuff
    }

    // 3.2.5
    // https://www.researchgate.net/publication/223808116_Implementation_and_analysis_of_the_BitTorrent_protocol_with_a_multi-agent_model
    // https://www.researchgate.net/figure/Implementation-of-the-choking-algorithm_fig3_223808116
    fn select_piece(torrent: &Torrent, worker: &peer_worker::Worker) {
        let pending_pieces = torrent.pieces.iter()
            .enumerate()
            .filter(|(i, piece)| piece.state == piece::State::Pending)
            .collect::<Vec<_>>();


        if torrent.completed_pieces() < 4 {
            // The peer will initially use a "random first piece" algorithm until it has four complete pieces

            let piece_i = pending_pieces
                .choose(&mut rand::thread_rng())
                .map(|(i, _)| *i)
                .unwrap();

            //let piece_i = rand::thread_rng().gen_range(0..pending_pieces.len());
            worker.set_state(peer_worker::State::Download(piece_i));
        } else {
            // When a peer gets at least four pieces, it switches the algorithm to "rarest piece first".
            // The policy is determining the rarest pieces in the own peer set and download those ﬁrst, so the peer will have more unusual pieces, which will be helpful in the trade with other peers.
            let piece_i = pending_pieces
                .tap_mut(|pieces| pieces.sort_by_key(|(i, piece)| piece.availability))
                .choose(&mut rand::thread_rng())
                .map(|(i, _)| *i)
                .unwrap();
            
            worker.set_state(peer_worker::State::Download(piece_i));
        }

        todo!("Not done")
    }
}
