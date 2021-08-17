// https://discord.com/channels/442252698964721669/443150878111694848/876796316901068871

use std::{sync::{Arc, Mutex}};

use crate::{metainfo::{MetaInfo}, tracker::Tracker};

type EventListener = Box<dyn FnMut(Arc<Mutex<SessionState>>, &Event)>;

pub struct EventDispatcher {
	listeners: Vec<EventListener>
}

impl EventDispatcher {
	pub fn dispatch(&mut self, session: Arc<Mutex<SessionState>>, event: Event) {
		for listener in self.listeners.iter_mut() {
			listener(session.clone(), &event);
		}
	}

	pub fn add_listener(&mut self, listener: EventListener) {
		self.listeners.push(listener);
	}
}

pub struct SessionState {
	torrents: Vec<Torrent>
}

pub struct Session {
	id_counter: u32,
	state: Arc<Mutex<SessionState>>,
	dispatcher: EventDispatcher
}

impl Session {
	pub fn new() -> Self {
		let dispatcher = EventDispatcher {
			listeners: vec![Box::new(Session::on)]
		};
	
		let session = Session {
			id_counter: 1,
			state: Arc::new(Mutex::new(SessionState {
				torrents: Vec::new()
			})),
			dispatcher
		};
	
		return session;
	}

	pub fn add(&mut self, meta_info: MetaInfo) {
		let torrent = Torrent {
			id: self.id_counter,
			trackers: vec![Tracker::new(&meta_info.tracker)],
			pieces: Vec::new(),
			meta_info
		};

		let event = Event::TorrentAdded(torrent.id);

		self.id_counter += 1;
		self.state.lock().unwrap().torrents.push(torrent);
		
		self.dispatcher.dispatch(self.state.clone(), event);
	}

	fn on(state: Arc<Mutex<SessionState>>, event: &Event) {
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
	Idle, Downloading, Completed
}

pub enum Event {
	TorrentAdded(u32)
}
