use std::sync::Arc;

use tokio::{sync::Mutex, task::JoinError};

use crate::{metainfo::MetaInfo, bitfield::Bitfield, tracker::{Tracker, Announce}, peer::{Peer, Message}};

type TrackerPtr = Arc<Mutex<Tracker>>;
type TorrentPtr = Arc<Mutex<Torrent>>;

struct Work {
	torrent: TorrentPtr,
	piece: usize
}

pub struct Session {
	peer_id: [u8; 20],
	trackers: Vec<TrackerPtr>,
	torrents: Vec<TorrentPtr>,
	work_queue: Arc<Mutex<Vec<Work>>>
}

impl Session {
	pub fn new(peer_id: [u8; 20]) -> Session {
		Session {
			peer_id,
			trackers: Vec::new(),
			torrents: Vec::new(),
			work_queue: Arc::new(Mutex::new(Vec::new()))
		}
	}

	pub async fn add(&mut self, info: MetaInfo) {
		// TODO: Don't allow duplicate trackers
		// if let None = self.trackers.iter().find(async move |tracker| tracker.lock().await.announce_url() == info.tracker) {}

		let tracker = Arc::new(Mutex::new(Tracker::new(&info.tracker)));
		let torrent = Arc::new(Mutex::new(Torrent {
			tracker: tracker.clone(),
			info: info.clone(),
			pieces: Bitfield::new(info.pieces.len()),
			store: Box::new(MemoryStore::new(info.files.iter().fold(0, |acc, file| acc + file.length)))
		}));

		self.trackers.push(tracker.clone());
		self.torrents.push(torrent.clone());

		let response = tracker.lock().await.announce(&Announce {
			info_hash: info.info_hash,
			peer_id: self.peer_id,
			ip: None,
			port: 8000,
			uploaded: 0,
			downloaded: 0,
			left: 0,
			event: None
		}).await.unwrap();

		for peer_addr in response.peers_addrs {
			let work_queue = self.work_queue.clone();
			let peer_id = self.peer_id.clone();
			let info = info.clone();

			tokio::spawn(async move {
				let mut peer = Peer::connect(peer_addr, &info, &peer_id).await.unwrap();

				peer.send_message(Message::Unchoke).await.unwrap();
				peer.send_message(Message::Interested).await.unwrap();
				
				loop {
					let work = work_queue.lock().await.pop().unwrap();

					peer.receive_message()
				}
			});
		}
	}
}

#[derive(Debug)]
pub struct Torrent {
	tracker: Arc<Mutex<Tracker>>,
	info: MetaInfo,
	pieces: Bitfield,
	store: Box<dyn Store>
}

pub trait Store: std::fmt::Debug + Send {
	fn get(&self, begin: usize, length: usize) -> &[u8];
	fn set(&mut self, begin: usize, data: &[u8]);
}

#[derive(Debug)]
pub struct MemoryStore {
	data: Vec<u8>
}

impl MemoryStore {
	pub fn new(length: usize) -> MemoryStore {
		MemoryStore {
			data: vec![0; length]
		}
	}
}

impl Store for MemoryStore {
    fn get(&self, i: usize, length: usize) -> &[u8] {
        return &self.data[i..(i + length)];
    }

    fn set(&mut self, begin: usize, data: &[u8]) {
        for i in 0..data.len() {
			self.data[begin + i] = data[i];
		}
    }
}
