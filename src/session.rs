use std::{collections::VecDeque, sync::Arc, time::{Duration, Instant}, convert::{TryFrom}};

use sha1::{Digest, Sha1};
use tokio::{
	net::TcpStream,
	sync::{
		mpsc::{Receiver, Sender},
		Mutex
	}
};

use crate::{
	metainfo::MetaInfo,
	peer::{Peer, PeerError},
	store::Store,
	tracker::{Announce, Tracker},
	wire::{Handshake, Message}
};

type TrackerPtr = Arc<Mutex<Tracker>>;
type TorrentPtr = Arc<Mutex<Torrent>>;

const BLOCK_SIZE: usize = 16_384;
const TIMEOUT: Duration = Duration::from_secs_f64(30.0);
const VERIFY_STORE: bool = true;

#[derive(Debug)]
struct Work {
	torrent: TorrentPtr,
	piece: usize,
	length: usize
}

#[derive(Debug, Clone, PartialEq)]
enum State {
	Pending,
	Downloaing,
	Downloaded,
	Verifying,
	Done
}

#[derive(Debug)]
pub struct Torrent {
	tracker: TrackerPtr,
	info: MetaInfo,
	peers: Vec<Peer>,
	pieces: Vec<State>, // Bitfield
	store: Box<dyn Store>
}

pub struct Session {
	peer_id: [u8; 20],
	// dispatcher: EventDispatcher,
	tx: Sender<Event>,
	rx: Receiver<Event>,

	trackers: Vec<TrackerPtr>,
	_torrents: Vec<TorrentPtr>,

	// Should be sorted by a priority byte
	work_queue: Arc<Mutex<VecDeque<Work>>>
}

/// The maximum amount of simultaneous requests that can be sent to a peer
const MAX_REQUESTS: usize = 5;

#[derive(Debug)]
enum Event {
	/// Torrent added to the session
	TorrentAdded(TorrentPtr),

	/// Piece(s) added to the work queue
	PieceAdded,

	/// Piece received from a peer
	PieceReceived(Work, Vec<u8>),

	/// Piece sha1 hash verified
	PieceVerified(Work, Vec<u8>),

	/// Piece sha1 hash failed to verify
	PieceNotVerified(Work, Vec<u8>)
}

enum DownloadError {
	Timeout,
	Choked,
	MessageError,
	PeerError(PeerError)
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

impl Session {
	pub fn new(peer_id: [u8; 20]) -> Self {
		let (tx, rx) = tokio::sync::mpsc::channel(32);

		let session = Session {
			peer_id,
			tx,
			rx,
			trackers: Vec::new(),
			_torrents: Vec::new(),
			work_queue: Arc::new(Mutex::new(VecDeque::new()))
		};

		return session;
	}

	pub async fn add(&mut self, info: MetaInfo, store: Box<dyn Store>) {
		let mut pieces = vec![State::Pending; info.pieces.len()];

		let store = Arc::new(Mutex::new(store));

		if VERIFY_STORE {
			for (i, v) in verify_store(&info, store.clone()).await.into_iter().enumerate() {
				if v {
					pieces[i] = State::Done;
				}
			}

			println!(
				"Store verified. {}/{} pieces already done.",
				pieces.iter().filter(|p| p == &&State::Done).count(),
				pieces.len()
			);
		}

		let tracker = self
			.trackers
			.iter()
			.find(|tracker| tracker.blocking_lock().announce_url() == &info.announce)
			.cloned()
			.unwrap_or_else(|| Arc::new(Mutex::new(Tracker::new(&info.announce))));

		let torrent = Arc::new(Mutex::new(Torrent {
			tracker,
			info: info.clone(),
			peers: Vec::new(),
			pieces, // Bitfield::new(info.pieces.len())
			store: Arc::try_unwrap(store).unwrap().into_inner()
		}));

		let event = Event::TorrentAdded(torrent);

		self.tx.send(event).await.expect("Failed to send");
	}

	pub async fn poll_events(&mut self) {
		let event = self.rx.recv().await.unwrap();

		match event {
			Event::TorrentAdded(torrent) => {
				println!("Event::TorrentAdded {}", torrent.lock().await.info.name);

				self.add_torrent_work(torrent.clone()).await;
				self.add_torrent_peers(torrent.clone()).await;
			},
			Event::PieceAdded => {
				println!("Event::PieceAdded");

				println!(
					"{:#?}",
					self.work_queue
						.lock()
						.await
						.iter()
						.map(|w| w.piece)
						.collect::<Vec<_>>()
				);
			},
			Event::PieceReceived(work, data) => {
				println!("Event::PieceReceived {}", work.piece);

				let tx = self.tx.clone();

				tokio::spawn(async move {
					work.torrent.lock().await.pieces[work.piece] = State::Verifying;

					let hash = work.torrent.lock().await.info.pieces[work.piece];

					if verify_piece(&data, &hash) {
						tx.send(Event::PieceVerified(work, data)).await.unwrap();
					} else {
						tx.send(Event::PieceNotVerified(work, data)).await.unwrap();
					}
				});
			},
			Event::PieceVerified(work, data) => {
				println!("Event::PieceVerified {}", work.piece);

				let mut lock = work.torrent.lock().await;
				lock.pieces[work.piece] = State::Done;

				let piece_length = lock.info.piece_size;
				lock.store
					.set(work.piece * piece_length, &data)
					.expect("Failed to write to store");

				let none_count = lock.pieces.iter().filter(|p| p == &&State::Pending).count();
				let downloading_count = lock
					.pieces
					.iter()
					.filter(|p| p == &&State::Downloaing)
					.count();
				let done_count = lock.pieces.iter().filter(|p| p == &&State::Done).count();
				let total_count = lock.pieces.len();

				println!(
					"None: {}, Downloading: {}, Done: {}, Total: {}",
					none_count, downloading_count, done_count, total_count
				);
			},
			Event::PieceNotVerified(work, _data) => {
				// TODO: Only if we have like a broadcaster, this would be in order to notify the peer threads
				// tx.send(Event::WorkAdded);

				eprintln!("Failed to verify piece {}", work.piece);

				self.work_queue.lock().await.push_back(work);
			}
		}
	}

	async fn add_torrent_peers(&mut self, torrent: TorrentPtr) {
		println!("Adding peers for torrent...");

		let torrent_lock = torrent.lock().await;

		let announce = torrent_lock
			.tracker
			.lock()
			.await
			.announce(&Announce {
				info_hash: torrent_lock.info.info_hash,
				peer_id: self.peer_id,
				ip: None,
				port: 8000,
				uploaded: 0,
				downloaded: 0,
				left: 0,
				event: None
			})
			.await
			.expect("Failed to announce");

		for addr in announce.peers_addrs {
			let info = torrent_lock.info.clone();
			let peer_id = self.peer_id;
			let work_queue = self.work_queue.clone();

			let tx = self.tx.clone();
			let torrent = torrent.clone();

			tokio::spawn(async move {
				let result = match tokio::time::timeout(TIMEOUT, TcpStream::connect(addr)).await {
					Ok(v) => v,
					Err(_) => return
				};

				let stream = match result {
					Ok(v) => v,
					Err(_) => {
						eprintln!("Error: Failed to connect to peer");
						return;
					}
				};

				let result = Peer::handshake(
					stream,
					Handshake {
						extensions: [0; 8],
						info_hash: info.info_hash,
						peer_id
					}
				)
				.await;

				let mut peer = match result {
					Ok(v) => v,
					Err(_) => {
						eprintln!("Error: Handshake failed");
						return;
					}
				};

				//.expect("Handshake failed");

				// https://github.com/veggiedefender/torrent-client/blob/2bde944888e1195e81cc5d5b686f6ec3a9f08c25/p2p/p2p.go#L133

				// peer.send_message(Message::Unchoke).await.expect("Failed to send Message::Unchoke");
				peer.send(Message::Interested)
					.await
					.expect("Failed to send Message::Interested");

				loop {
					if peer.peer_choking() {
						// 1. Wait until we are unchoked
						peer.receive()
							.await
							.expect("Failed to receive message (while waiting for unchoke)");
					} else {
						// 2. As long as we are unchoked, do work from the queue

						// println!("Grabbing work");
						let work = work_queue.lock().await.pop_front();

						if let Some(work) = work {
							if peer.has_piece(work.piece) {
								torrent.lock().await.pieces[work.piece] = State::Downloaing;

								println!("Downloading piece {}", work.piece);

								match Session::get_piece(&mut peer, &work).await {
									Ok(data) => {
										torrent.lock().await.pieces[work.piece] = State::Downloaded;
										tx.send(Event::PieceReceived(work, data)).await.unwrap();
									},
									Err(err) => {
										println!("Failed to download piece {}", work.piece);

										torrent.lock().await.pieces[work.piece] = State::Pending;
										work_queue.lock().await.push_back(work);

										match err {
											DownloadError::Timeout => {
												eprintln!("Connection timed out");
												break;
											},
											DownloadError::PeerError(error) => {
												eprintln!("A peer error occurred: {:#?}", error);
												break;
											},
											_ => ()
										}
									}
								}
							} else {
								work_queue.lock().await.push_back(work);
							}
						} else {
							// println!("No work available");
							// break;
							tokio::time::sleep(Duration::from_secs_f64(5.0)).await;
						}
					}
				}

				println!("Severing connection");
			});
		}
	}

	async fn add_torrent_work(&mut self, torrent: TorrentPtr) {
		println!("Adding work for torrent...");

		let torrent_lock = torrent.lock().await;

		// Add work.
		// TODO: Right now it's added in order, but we should probably randomize the work instead
		self.work_queue.lock().await.extend(
			(0..torrent_lock.info.pieces.len())
				.filter(|i| torrent_lock.pieces[*i] == State::Pending)
				.map(|i| Work {
					torrent: torrent.clone(),
					piece: i,
					length: if i == torrent_lock.info.pieces.len() - 1 {
						torrent_lock.info.last_piece_size
					} else {
						torrent_lock.info.piece_size
					}
				})
		);

		self.tx.send(Event::PieceAdded).await.unwrap();
	}

	// TODO: We might want to stop "counting requests" and use a channel instead. Maybe start another "listening thread" and use a channel to .recv the pieces (incoming pieces only channel, basically)
	async fn get_piece(peer: &mut Peer, work: &Work) -> Result<Vec<u8>, DownloadError> {
		let mut blocks = Vec::new();

		let mut active_requests = 0;

		for i in (0..work.length).step_by(BLOCK_SIZE) {
			let block_size = (work.length - i).min(BLOCK_SIZE);

			// println!("Requesting piece {} block {}", work.piece, i);
			tokio::time::timeout(
				TIMEOUT,
				peer.send(Message::Request(
					work.piece as u32,
					i as u32,
					block_size as u32
				))
			)
			.await??;

			active_requests += 1;

			while active_requests >= MAX_REQUESTS {
				match tokio::time::timeout(TIMEOUT, peer.receive()).await?? {
					Message::Piece(_index, begin, block) => {
						// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
						blocks.push((begin, block));
						active_requests -= 1;
					},
					Message::Choke => {
						return Err(DownloadError::Choked);
					},
					Message::KeepAlive | Message::Unchoke => (),
					msg => {
						// TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
						eprintln!("Unexpected message (while receiving piece): {:?}", msg);
						return Err(DownloadError::MessageError);
					}
				}
			}
		}

		while active_requests > 0 {
			match tokio::time::timeout(TIMEOUT, peer.receive()).await?? {
				Message::Piece(_index, begin, block) => {
					// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
					blocks.push((begin, block));
					active_requests -= 1;
				},
				Message::Choke => {
					return Err(DownloadError::Choked);
				},
				Message::KeepAlive | Message::Unchoke => (),
				msg => {
					// TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
					eprintln!("Unexpected message (while receiving piece): {:?}", msg);
					return Err(DownloadError::MessageError);
				}
			}
		}

		// Assemble the blocks into a piece
		println!("Assembling piece {}", work.piece);
		blocks.sort_by(|(a, _), (b, _)| a.cmp(&b));
		let data = blocks
			.into_iter()
			.map(|(_offset, data)| data)
			.flatten()
			.collect::<Vec<_>>();

		Ok(data)
	}
}

fn verify_piece(piece: &[u8], hash: &[u8; 20]) -> bool {
	hash == &<[u8; 20]>::try_from(Sha1::digest(&piece).as_slice()).unwrap()
}

async fn verify_store(info: &MetaInfo, store: Arc<Mutex<Box<dyn Store>>>) -> Vec<bool> {
	let pieces = Arc::new(std::sync::Mutex::new(vec![false; info.pieces.len()]));

	let iter = Arc::new(std::sync::Mutex::new(info.pieces.clone().into_iter().enumerate()));
	
	let threads = (num_cpus::get() - 1).max(1);

	println!("Starting {} threads...", threads);
		
	let handles = (0..threads).map(|_i| {
		let info = info.clone();
		let iter = iter.clone();
		let store = store.clone();
		let pieces = pieces.clone();

		std::thread::spawn(move || {
			// TODO: Might lock during the entire loop, watch out!
			// let mut last = Instant::now();

			// Some((i, hash))
			let mut v;

			loop {
				v = iter.lock().unwrap().next();

				if v == None {
					break;
				}

				let (i, hash) = v.unwrap();

				// let begin = Instant::now();

				let data = store.blocking_lock()
					.get(
						i * info.piece_size,
						if i == info.pieces.len() - 1 {
							info.last_piece_size
						} else {
							info.piece_size
						}
					)
					.expect("Failed to get data from store");
				
				// let data_dur = Instant::now() - begin;

				// let begin = Instant::now();
				
				if verify_piece(&data, &hash) {
					pieces.lock().unwrap()[i] = true;
				}
				// let verify_dur = Instant::now() - begin;

				// let tot = data_dur + verify_dur;

				// let end = Instant::now();

				// println!(
				// 	"{}.\tPiece {} verified in {}ns ({}ns/{:.1}% + {}ns/{:.1}%). Misc. time: {}ns/{:.1}%",
				// 	ti,
				// 	i,
				// 	tot.as_nanos(),
				// 	data_dur.as_nanos(),
				// 	100.0 * data_dur.as_secs_f64() / tot.as_secs_f64(),
				// 	verify_dur.as_nanos(),
				// 	100.0 * verify_dur.as_secs_f64() / tot.as_secs_f64(),
				// 	(end - last - tot).as_nanos(),
				// 	(end - last - tot).as_secs_f64() / (end - last).as_secs_f64()
				// );

				// last = end;
			}
		})
	}).collect::<Vec<_>>();

	println!("Verifying pieces...");

	for handle in handles {
		handle.join().unwrap();
	}

	Arc::try_unwrap(pieces).unwrap().into_inner().unwrap()
}
