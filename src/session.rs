use std::{sync::{Arc}};

use futures::Future;
use tokio::sync::Mutex;

use crate::{metainfo::{MetaInfo}, tracker::Tracker, bitfield::Bitfield, store::{Store, MemoryStore}};

type EventListener = Box<dyn Fn(&Session, &EventDispatcher, &Event)>;
type TrackerPtr = Arc<Mutex<Tracker>>;
type TorrentPtr = Arc<Mutex<Torrent>>;

pub struct EventDispatcher {
	listeners: Vec<EventListener>
}

impl EventDispatcher {
	pub fn dispatch(&self, session: &Session, event: Event) {
		for listener in self.listeners.iter() {
			listener(session, self, &event);
		}
	}

	pub fn add_listener(&mut self, listener: EventListener) {
		self.listeners.push(listener);
	}
}

struct Work {
	torrent: Torrent,
	piece: usize
}

#[derive(Debug)]
pub struct Torrent {
	tracker: Arc<Mutex<Tracker>>,
	info: MetaInfo,
	pieces: Bitfield,
	store: Box<dyn Store>
}

pub struct Session {
	peer_id: [u8; 20],
	dispatcher: EventDispatcher,

	trackers: Vec<TrackerPtr>,
	torrents: Vec<TorrentPtr>,
	work_queue: Arc<Mutex<Vec<Work>>>
}

#[derive(Clone)]
pub enum Event {
	TorrentAdded(TorrentPtr)
}

impl Session {
	pub fn new(peer_id: [u8; 20]) -> Self {
		let dispatcher = EventDispatcher {
			listeners: vec![Box::new(Session::on)]
		};
	
		let session = Session {
			peer_id,
			dispatcher,
    		trackers: Vec::new(),
			torrents: Vec::new(),
			work_queue: Arc::new(Mutex::new(Vec::new()))
		};
	
		return session;
	}

	pub async fn add(&mut self, info: MetaInfo) {
		let tracker = Arc::new(Mutex::new(Tracker::new(&info.announce)));

		let torrent = Arc::new(Mutex::new(Torrent {
			tracker: tracker.clone(),
			info: info.clone(),
			pieces: Bitfield::new(info.pieces.len()),
			store: Box::new(MemoryStore::new(info.files.iter().fold(0, |acc, file| acc + file.length)))
		}));

		let event = Event::TorrentAdded(torrent);
		
		self.dispatcher.dispatch(&self, event);
	}

	fn on(_session: &Session, _dispatcher: &EventDispatcher, event: &Event) {
		let event = event.clone();

		println!("On event");
		
		println!("Matching event...");

		match event {
			Event::TorrentAdded(torrent) => {
				println!("Torrent {} added", torrent.lock().await.info.name);
			}
		}

		tokio::run()
	}
}
