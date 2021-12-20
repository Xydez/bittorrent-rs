use std::{sync::{Arc}};

use tokio::sync::Mutex;

use crate::{metainfo::{MetaInfo}, tracker::{Tracker, Announce}, bitfield::Bitfield, store::{Store, MemoryStore}};

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
	torrent: TorrentPtr,
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
	TorrentAdded(TorrentPtr),
	WorkAdded
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

	fn on(session: &Session, dispatcher: &EventDispatcher, event: &Event) {
		let event = event.clone();

		let peer_id = session.peer_id;
		let work_queue = session.work_queue.clone();

		tokio::spawn(async move {
			println!("Matching event...");

			match event {
				Event::TorrentAdded(torrent) => {
					let torrent_lock = torrent.lock().await;
					println!("Torrent {} added", torrent_lock.info.name);

					let tracker_lock = torrent_lock.tracker.lock().await;

					let announce = tracker_lock.announce(&Announce {
						info_hash: torrent_lock.info.info_hash,
						peer_id,
						ip: None,
						port: 8000,
						uploaded: 0,
						downloaded: 0,
						left: 0,
						event: None
					}).await.unwrap();

					// Add work. Right now it's added in order, but we should probably randomize the work instead
					work_queue.lock().await.extend((0..torrent_lock.info.pieces.len())
						.map(|i| Work { torrent: torrent.clone(), piece: i })
					);
				},
				_ => ()
			}
		});
	}
}
