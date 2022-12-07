//! Events that can occur in a [`Session`](crate::core::session::Session)
//!
//! # Rules
//! * No synchronization primitives allowed. Use identifiers instead.

use std::sync::Arc;

use crate::{
	core::{piece::PieceId, torrent::TorrentId},
	protocol::tracker
};

/// Events for a session
#[derive(Debug, Clone)]
pub enum Event {
	/// Session is starting
	Started,
	/// Session is stopping
	Stopped,
	/// Event for a torrent in the session
	TorrentEvent(TorrentId, TorrentEvent)
}

/// Events for a torrent
#[derive(Debug, Clone)]
pub enum TorrentEvent {
	/// Torrent added to the session
	Added,
	/// Torrent has finished downloading
	Done,
	/// Torrent has been announced to the tracker
	Announced(Arc<tracker::Response>),
	/// Event for a piece in the torrent
	PieceEvent(PieceId, PieceEvent)
}

/// Events for a piece of a torrent
#[derive(Debug, Clone)]
pub enum PieceEvent {
	/// Block of a piece has been received
	Block(usize),

	/// Piece has been downloaded
	Downloaded(Arc<Vec<u8>>),

	/// Piece has been verified
	Verified(Arc<Vec<u8>>), // TODO: Since the piece is in the store we might not need to include it here? Anyways just a useless thought

	/// Piece has been stored
	Done
}

impl std::fmt::Display for PieceEvent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			PieceEvent::Block(block) => write!(f, "Block({block})"),
			PieceEvent::Downloaded(data) => write!(f, "Downloaded({} bytes)", data.len()),
			PieceEvent::Verified(data) => write!(f, "Verified({} bytes)", data.len()),
			PieceEvent::Done => write!(f, "Done")
		}
	}
}

impl std::fmt::Display for TorrentEvent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			TorrentEvent::Added => write!(f, "Added"),
			TorrentEvent::Done => write!(f, "Done"),
			TorrentEvent::Announced(response) => {
				write!(f, "Announced({} peers)", response.peers_addrs.len())
			}
			TorrentEvent::PieceEvent(piece, event) => write!(f, "PieceEvent({piece}, {event})")
		}
	}
}

impl std::fmt::Display for Event {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Event::Started => write!(f, "Started"),
			Event::Stopped => write!(f, "Stopped"),
			Event::TorrentEvent(torrent, event) => {
				write!(f, "TorrentEvent({torrent}, {event})")
			}
		}
	}
}
