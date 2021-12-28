use std::{sync::{Arc}, time::Duration, collections::VecDeque};

use tokio::{sync::{Mutex, mpsc::{Receiver, Sender}}, net::TcpStream};

use crate::{metainfo::MetaInfo, tracker::{Tracker, Announce}, store::{Store, MemoryStore}, peer::{Peer}, wire::{Handshake, Message}};

// type EventListener = Box<dyn Fn(&Session, &EventDispatcher, &Event)>;
type TrackerPtr = Arc<Mutex<Tracker>>;
type TorrentPtr = Arc<Mutex<Torrent>>;

const BLOCK_SIZE: usize = 16_384;
const TIMEOUT: Duration = Duration::from_secs_f64(3.0);

// pub struct EventDispatcher {
// 	event_queue: Arc<Vec<Event>>,
// 	listeners: Vec<EventListener>
// }

// impl EventDispatcher {
// 	pub fn push(&mut self, event: Event) {
// 		self.event_queue.push(event);
// 	}

// 	pub fn dispatch(&mut self, session: &Session) {
// 		for event in self.event_queue.iter() {
// 			for listener in self.listeners.iter() {
// 				listener(session, self, &event);
// 			}
// 		}
// 	}

// 	pub fn add_listener(&mut self, listener: EventListener) {
// 		self.listeners.push(listener);
// 	}
// }

struct Work {
	torrent: TorrentPtr,
	piece: usize,
	length: usize
}

#[derive(Debug, Clone, PartialEq)]
enum State {
	None,
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
	tx: Sender<Event>, rx: Receiver<Event>,

	_trackers: Vec<TrackerPtr>,
	_torrents: Vec<TorrentPtr>,
	work_queue: Arc<Mutex<VecDeque<Work>>>
}

/// The maximum amount of simultaneous requests that can be sent to a peer
const MAX_REQUESTS: usize = 5;

#[derive(Debug, Clone)]
pub enum Event {
	TorrentAdded(TorrentPtr),
	WorkAdded,
	PieceReceived(TorrentPtr, usize, Vec<u8>),
	PieceVerified(TorrentPtr, usize, Vec<u8>)
}

impl Session {
	pub fn new(peer_id: [u8; 20]) -> Self {
		let (tx, rx)  = tokio::sync::mpsc::channel(32);

		let session = Session {
			peer_id,
			tx, rx,
    		_trackers: Vec::new(),
			_torrents: Vec::new(),
			work_queue: Arc::new(Mutex::new(VecDeque::new()))
		};
	
		return session;
	}

	pub async fn add(&mut self, info: MetaInfo) {
		let tracker = Tracker::new(&info.announce);

		let tracker = Arc::new(Mutex::new(tracker));

		let torrent = Arc::new(Mutex::new(Torrent {
			tracker: tracker,
			info: info.clone(),
			peers: Vec::new(),
			pieces: vec![State::None; info.pieces.len()], // Bitfield::new(info.pieces.len())
			store: Box::new(MemoryStore::new(info.files.iter().fold(0, |acc, file| acc + file.length)))
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
			Event::WorkAdded => {
				println!("Event::WorkAdded");
			},
			Event::PieceReceived(torrent, piece, data) => {
				println!("Event::PieceReceived {}", piece);
				
				torrent.lock().await.pieces[piece] = State::Verifying;

				let tx = self.tx.clone();

				tokio::spawn(async move {
					// For now, just instantly verify the piece
					tx.send(Event::PieceVerified(torrent, piece, data)).await.unwrap();
				});

			},
			Event::PieceVerified(torrent, piece, data) => {
				println!("Event::PieceVerified {}", piece);

				let mut lock = torrent.lock().await;
				lock.pieces[piece] = State::Done;

				let piece_length = lock.info.piece_length;
				lock.store.set(piece * piece_length, &data);

				let none_count = lock.pieces.iter().filter(|p| p == &&State::None).count();
				let downloading_count = lock.pieces.iter().filter(|p| p == &&State::Downloaing).count();
				let done_count = lock.pieces.iter().filter(|p| p == &&State::Done).count();
				let total_count = lock.pieces.len();

				println!("None: {}, Downloading: {}, Done: {}, Total: {}", none_count, downloading_count, done_count, total_count);
			}
		}
	}

	async fn add_torrent_peers(&mut self, torrent: TorrentPtr) {
		println!("Adding peers for torrent...");

		let torrent_lock = torrent.lock().await;
		let tracker_lock = torrent_lock.tracker.lock().await;

		let announce = tracker_lock.announce(&Announce {
			info_hash: torrent_lock.info.info_hash,
			peer_id: self.peer_id,
			ip: None,
			port: 8000,
			uploaded: 0,
			downloaded: 0,
			left: 0,
			event: None
		}).await.unwrap();

		for addr in announce.peers_addrs {
			let info = torrent_lock.info.clone();
			let peer_id = self.peer_id;
			let work_queue = self.work_queue.clone();

			let tx = self.tx.clone();
			let torrent = torrent.clone();

			tokio::spawn(async move {
				let stream = match tokio::time::timeout(
					Duration::from_secs_f64(3.0),
					TcpStream::connect(addr)
				).await {
					Ok(v) => v,
					Err(_) => return
				}.unwrap();

				let mut peer = Peer::handshake(stream, Handshake {
    			    extensions: [0; 8],
    			    info_hash: info.info_hash,
    			    peer_id,
    			}).await.unwrap();

				// https://github.com/veggiedefender/torrent-client/blob/2bde944888e1195e81cc5d5b686f6ec3a9f08c25/p2p/p2p.go#L133

				// peer.send_message(Message::Unchoke).await.unwrap();
				peer.send(Message::Interested).await.unwrap();

				loop {
					if peer.peer_choking() {
						// 1. Wait until we are unchoked
						peer.receive().await.unwrap();
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
										tx.send(Event::PieceReceived(work.torrent, work.piece, data)).await.unwrap();
									},
									Err(_) => {
										torrent.lock().await.pieces[work.piece] = State::None;
										work_queue.lock().await.push_back(work);
									}
								}
							} else {
								work_queue.lock().await.push_back(work);
							}

						} else {
							println!("No work available");
							break;
						}
					}
				}

				println!("Ending peer connection");
			});
		}
	}

	async fn add_torrent_work(&mut self, torrent: TorrentPtr) {
		println!("Adding work for torrent...");

		let torrent_lock = torrent.lock().await;

		// Add work. Right now it's added in order, but we should probably randomize the work instead
		self.work_queue.lock().await.extend((0..torrent_lock.info.pieces.len())
			.map(|i| Work { torrent: torrent.clone(), piece: i, length: torrent_lock.info.piece_length })
		);

		self.tx.send(Event::WorkAdded).await.unwrap();
	}

	//	TODO: loop {
	//		<wait for unchoke>
	//		while <unchoked> {
	//			<grab work>
	//			<get piece>
	//		}
	//	}
	// TODO: Create error type (e.g. Error::Choke should rerun the loop, Error::Timeout should sever the connection)
	// TODO: We really need to stop "counting requests" and use a channel instead. Maybe start another "listening thread" and use a channel to .recv the pieces (incoming pieces only channel, basically)
	async fn get_piece(peer: &mut Peer, work: &Work) -> Result<Vec<u8>, ()> {
		let mut blocks = Vec::new();

		let mut active_requests = 0;

		for i in (0..work.length).step_by(BLOCK_SIZE) {
			let block_size = (work.length - i).min(BLOCK_SIZE);

			// println!("Requesting piece {} block {}", work.piece, i);
			tokio::time::timeout(TIMEOUT, peer.send(Message::Request(work.piece as u32, i as u32, block_size as u32))).await.unwrap().unwrap();
			active_requests += 1;

			while active_requests >= MAX_REQUESTS {
				match tokio::time::timeout(Duration::from_secs_f64(3.0), peer.receive()).await {
					Ok(Ok(msg)) => match msg {
						Message::Piece(_index, begin, block) => {
							// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
							blocks.push((begin, block));
							active_requests -= 1;
						},
						// TODO: In the future we might have two queues, one for received pieces and one for requested pieces, and we have two separate threads that keep on requesting and sending pieces.
						_ => ()
					},
					Ok(Err(err)) => {
						eprintln!("{:#?}", err);
						return Err(());
					},
					Err(_) => {
						eprintln!("Request timed out");
						return Err(());
					}
				}
			}

			while active_requests > 0 {
				match tokio::time::timeout(Duration::from_secs_f64(3.0), peer.receive()).await {
					Ok(Ok(msg)) => match msg {
						Message::Piece(_index, begin, block) => {
							// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
							blocks.push((begin, block));
							active_requests -= 1;
						},
						Message::Choke => {
							return Err(());
						},
						Message::Unchoke => (), // Since we are already unchoked, this should not do anything
						msg => panic!("{:?}", msg) // Temporary
					},
					Ok(Err(err)) => {
						eprintln!("{:#?}", err);
						return Err(());
					},
					Err(_) => {
						eprintln!("Request timed out");
						return Err(());
					}
				}
			}
		}

		// Assemble the blocks into a piece
		println!("Assembling piece {}", work.piece);
		blocks.sort_by(|(a, _), (b, _)| a.cmp(&b));
		let data = blocks.into_iter().map(|(_offset, data)| data).flatten().collect::<Vec<_>>();
		
		Ok(data)
	}
}
