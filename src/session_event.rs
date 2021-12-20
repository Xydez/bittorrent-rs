// https://discord.com/channels/442252698964721669/443150878111694848/876796316901068871

use std::{sync::{Arc, RwLock}};

use crate::{metainfo::{MetaInfo}, tracker::Tracker};

type EventListener = Box<dyn Fn(Arc<RwLock<SessionState>>, &EventDispatcher, &Event)>;

pub struct EventDispatcher {
	listeners: Vec<EventListener>
}

impl EventDispatcher {
	pub fn dispatch(&self, session: Arc<RwLock<SessionState>>, event: Event) {
		for listener in self.listeners.iter() {
			listener(session.clone(), self, &event); // session.clone()
		}
	}

	pub fn add_listener(&mut self, listener: EventListener) {
		self.listeners.push(listener);
	}
}

pub struct SessionState {
	id_counter: u32,
	torrents: Vec<Torrent>
}

pub struct Session {
	state: Arc<RwLock<SessionState>>,
	dispatcher: EventDispatcher
}

impl Session {
	pub fn new() -> Self {
		let dispatcher = EventDispatcher {
			listeners: vec![Box::new(Session::on)]
		};
	
		let session = Session {
			state: Arc::new(RwLock::new(SessionState {
				id_counter: 1,
				torrents: Vec::new()
			})),
			dispatcher
		};
	
		return session;
	}

	pub fn add(&mut self, meta_info: MetaInfo) {
		let torrent = Torrent {
			id: self.state.read().unwrap().id_counter,
			trackers: vec![Tracker::new(&meta_info.tracker)],
			pieces: Vec::new(),
			meta_info
		};

		let event = Event::TorrentAdded(torrent.id);

		{
			let lock = self.state.write().unwrap();
			lock.id_counter += 1;
			lock.torrents.push(torrent);
		}
		
		self.dispatcher.dispatch(self.state.clone(), event);
	}

	fn on(_state: Arc<RwLock<SessionState>>, _dispatcher: &EventDispatcher, event: &Event) {
		match event {
			Event::TorrentAdded(id) => {
				println!("Torrent {} added", id);
			}
		}
	}
}

#[derive(Debug)]
struct Torrent {
	id: u32,
	meta_info: MetaInfo,
	trackers: Vec<Tracker>,
	pieces: Vec<Piece>
}

#[derive(Debug)]
struct Piece {
	status: Status
}

#[derive(Debug)]
enum Status {
	// Idle, Downloading, Completed
}

#[derive(Clone)]
pub enum Event {
	TorrentAdded(u32)
}
