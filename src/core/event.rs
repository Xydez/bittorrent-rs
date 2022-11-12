use std::sync::Arc;

use strum::AsRefStr;

use crate::protocol::tracker;

use super::session::{TorrentPtr, PieceID};

// TODO: Make `Event` print properly

/// Events for a session
#[derive(Debug, AsRefStr)]
pub enum Event {
    /// Session is shutting down
    Shutdown,
    /// Event for a torrent in the session
    TorrentEvent(TorrentPtr, TorrentEvent),
}

/// Events for a torrent
#[derive(Debug, AsRefStr)]
pub enum TorrentEvent {
    /// Torrent added to the session
    Added,
    /// Torrent has finished downloading
    Done,
    /// Torrent has been announced to the tracker
    Announced(tracker::Response),
    /// Event for a piece in the torrent
    PieceEvent(PieceID, PieceEvent),
}

/// Events for individual pieces of a torrent
#[derive(Debug, AsRefStr)]
pub enum PieceEvent {
    Block(usize),

    /// Piece has been downloaded
    Downloaded(Arc<Vec<u8>>),

    /// Piece has been verified
    Verified(Arc<Vec<u8>>), // TODO: Since the piece is in the store we might not need to include it here? Anyways just a useless thought

    /// Piece has been stored
    Done,
}
