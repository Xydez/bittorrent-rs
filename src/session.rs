use std::{
	collections::VecDeque,
	convert::TryFrom,
	sync::Arc,
	time::{Duration, Instant}
};

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
	peer::Peer,
	peer_worker,
	// bitfield::Bitfield,
	store::Store,
	tracker::{Announce, Tracker},
	wire::Handshake
};

pub(crate) type TrackerPtr = Arc<Mutex<Tracker>>;
pub(crate) type TorrentPtr = Arc<Mutex<Torrent>>;

const ANNOUNCE_RETRIES: usize = 5;
const VERIFY_STORE: bool = true;

/// The time to wait for the peer to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs_f64(10.0);

#[derive(Debug)]
pub(crate) struct Work {
	pub torrent: TorrentPtr,
	pub piece: usize,
	pub length: usize
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum State {
	Pending,
	Downloaing,
	Downloaded,
	Verifying,
	Done
}

#[derive(Debug)]
pub struct Torrent {
	pub(crate) tracker: TrackerPtr,
	pub(crate) info: MetaInfo,
	pub(crate) pieces: Vec<State>,
	pub(crate) store: Box<dyn Store>
}

impl Torrent {
	pub fn done(&self) -> bool {
		self.pieces.iter().all(|p| p == &State::Done)
	}
}

pub struct Session {
	peer_id: [u8; 20],
	tx: Sender<Event>,
	rx: Receiver<Event>,

	trackers: Vec<TrackerPtr>,
	_torrents: Vec<TorrentPtr>,

	// Should be sorted by a priority byte in the future
	work_queue: Arc<Mutex<VecDeque<Work>>>
}

#[derive(Debug)]
pub(crate) enum Event {
	/// Torrent added to the session
	TorrentAdded(TorrentPtr),

	/// Torrent has finished downloading
	TorrentDone(TorrentPtr),

	/// Piece(s) added to the work queue
	PieceAdded,

	/// Piece received from a peer
	PieceReceived(Work, Vec<u8>),

	/// Piece sha1 hash verified
	PieceVerified(Work, Vec<u8>),

	/// Piece sha1 hash failed to verify
	PieceNotVerified(Work, Vec<u8>)
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

	pub async fn add(&mut self, info: MetaInfo, store: Box<dyn Store>) -> TorrentPtr {
		let mut pieces = vec![State::Pending; info.pieces.len()];

		let store = Arc::new(Mutex::new(store));

		if VERIFY_STORE {
			let begin = Instant::now();

			for (i, v) in verify_store(&info, store.clone())
				.await
				.into_iter()
				.enumerate()
			{
				if v {
					pieces[i] = State::Done;
				}
			}

			println!(
				"Store verified in {:.2}s. {}/{} pieces already done.",
				begin.elapsed().as_secs_f64(),
				pieces.iter().filter(|&p| p == &State::Done).count(),
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
			pieces,
			store: Arc::try_unwrap(store).unwrap().into_inner()
		}));

		self.tx
			.send(Event::TorrentAdded(torrent.clone()))
			.await
			.expect("Failed to send");

		torrent
	}

	pub async fn poll_events(&mut self) {
		let event = self.rx.recv().await.unwrap();

		match event {
			Event::TorrentAdded(torrent) => {
				println!("Event::TorrentAdded {}", torrent.lock().await.info.name);

				if torrent.lock().await.done() {
					self.tx
						.send(Event::TorrentDone(torrent.clone()))
						.await
						.unwrap();
				} else {
					// TODO: We should probably skip this until seeding is implemented
					self.add_torrent_work(torrent.clone()).await;
					self.add_torrent_peers(torrent.clone()).await;
				}
			},
			Event::TorrentDone(_torrent) => (),
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

				let pending_count = lock.pieces.iter().filter(|p| p == &&State::Pending).count();
				let downloading_count = lock
					.pieces
					.iter()
					.filter(|p| p == &&State::Downloaing)
					.count();
				let verifying_count = lock
					.pieces
					.iter()
					.filter(|p| p == &&State::Verifying)
					.count();
				let done_count = lock.pieces.iter().filter(|p| p == &&State::Done).count();
				let total_count = lock.pieces.len();

				println!(
					"Pending: {}, Downloading: {}, Verifying: {}, Done: {}, Total: {} ({} in queue)",
					pending_count, downloading_count, verifying_count, done_count, total_count, self.work_queue.lock().await.len()
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

		let announce = {
			let mut i = 0;

			let announce = Announce {
				info_hash: torrent_lock.info.info_hash,
				peer_id: self.peer_id,
				ip: None,
				port: 8000,
				uploaded: 0,
				downloaded: 0,
				left: 0,
				event: None
			};

			loop {
				if i < ANNOUNCE_RETRIES {
					let ret = torrent_lock
						.tracker
						.lock()
						.await
						.announce(&announce)
						.await
						.ok();

					if ret.is_some() {
						break ret;
					} else {
						eprintln!("Failed to announce. Retrying...")
					}
				} else {
					break None;
				}

				i += 1;
			}
		}
		.expect("Failed to announce");

		for addr in announce.peers_addrs {
			let info = torrent_lock.info.clone();
			let peer_id = self.peer_id;
			let work_queue = self.work_queue.clone();

			let tx = self.tx.clone();
			let torrent = torrent.clone();

			tokio::spawn(async move {
				let result =
					match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
						Ok(v) => v,
						Err(_) => {
							eprintln!("Error: Failed to connect to peer (Connection timed out)");
							return;
						}
					};

				let stream = match result {
					Ok(v) => v,
					Err(_) => {
						eprintln!("Error: Failed to connect to peer (Network error)");
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

				let peer = match result {
					Ok(v) => v,
					Err(_) => {
						eprintln!("Error: Failed to connect to peer (Handshake failed)");
						return;
					}
				};

				println!(
					"Connected to peer [{}]",
					crate::util::to_hex(peer.peer_id())
				);

				// Send bitfield?
				// peer.send(crate::wire::Message::Bitfield(bitfield_from_pieces(&torrent.lock().await.pieces)));

				peer_worker::run_worker(peer, work_queue, torrent, tx).await;
			});
		}
	}

	async fn add_torrent_work(&mut self, torrent: TorrentPtr) {
		println!("Adding work for torrent...");

		let torrent_lock = torrent.lock().await;

		// If there is no work to be done, return.
		if !torrent_lock.pieces.iter().any(|p| p == &State::Pending) {
			return;
		}

		// Add work.
		// TODO: Right now it's added in order, but we should probably either randomize the work or have a rarest-first system instead.
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
}

fn verify_piece(piece: &[u8], hash: &[u8; 20]) -> bool {
	hash == &<[u8; 20]>::try_from(Sha1::digest(&piece).as_slice()).unwrap()
}

async fn verify_store(info: &MetaInfo, store: Arc<Mutex<Box<dyn Store>>>) -> Vec<bool> {
	let pieces = Arc::new(std::sync::Mutex::new(vec![false; info.pieces.len()]));

	let iter = Arc::new(std::sync::Mutex::new(
		info.pieces.clone().into_iter().enumerate()
	));

	let threads = (num_cpus::get() - 1).max(1);

	println!("Verifying store using {} threads...", threads);

	let handles = (0..threads)
		.map(|_i| {
			let info = info.clone();
			let iter = iter.clone();
			let store = store.clone();
			let pieces = pieces.clone();

			std::thread::spawn(move || {
				let mut v;

				loop {
					v = iter.lock().unwrap().next();

					if v == None {
						break;
					}

					let (i, hash) = v.unwrap();

					let data = store
						.blocking_lock()
						.get(
							i * info.piece_size,
							if i == info.pieces.len() - 1 {
								info.last_piece_size
							} else {
								info.piece_size
							}
						)
						.expect("Failed to get data from store");

					if verify_piece(&data, &hash) {
						pieces.lock().unwrap()[i] = true;
					}
				}
			})
		})
		.collect::<Vec<_>>();

	for handle in handles {
		handle.join().unwrap();
	}

	Arc::try_unwrap(pieces).unwrap().into_inner().unwrap()
}

/*
fn bitfield_from_pieces(pieces: &Vec<State>) -> Bitfield {
	Bitfield::from_bytes(&pieces
		.chunks(8)
		.map(|s|
			s.iter()
				.enumerate()
				// TODO: Maybe it's not `1 << i` but `1 << (7 - i)`
				.fold(0u8, |acc, (i, v)| if v == &State::Done { acc + (1 << (7 - i)) } else { acc })
		)
		.collect::<Vec<u8>>()
	)
}
*/
