use std::{sync::Arc, collections::{BTreeMap, HashMap}};

use tokio::sync::Mutex;

use crate::{protocol::{metainfo::MetaInfo, tracker::Tracker}, io::store::Store};

use super::{piece::{Piece, Priority, State}, util, worker::Worker, algorithm::Picker, session::PieceID, piece_download::PieceDownload};

#[derive(Debug)]
pub struct Torrent {
    /// Meta info of the torrent
    pub meta_info: MetaInfo,
    /// List of all workers
    pub peers: Vec<Worker>,
    /// Store of the torrent
    pub store: Arc<Mutex<Box<dyn Store>>>,
    /// List of all pieces
    pub pieces: Vec<Piece>,
    /// Piece picker
    pub picker: Picker,
    /// Ongoing piece downloads
    pub downloads: HashMap<u32, Arc<Mutex<PieceDownload>>>,
    /// TODO: Implement multiple trackers
    pub tracker: Tracker
}

impl Torrent {
    pub fn new(meta_info: MetaInfo, store: Box<dyn Store>) -> Torrent {
        let mut pieces = Vec::new();

        for _ in 0..meta_info.pieces.len() {
            pieces.push(Piece::default());
        }

        let tracker = Tracker::new(&meta_info.announce);

        Torrent {
            meta_info,
            peers: Vec::new(),
            pieces,
            picker: Picker { end_game: false },
            store: Arc::new(Mutex::new(store)),
            downloads: HashMap::new(),
            tracker
        }
    }

    pub fn pending_pieces(&self) -> impl Iterator<Item = (PieceID, &Piece)> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.state == State::Pending)
            .map(|(i, p)| (i as PieceID, p))
    }

    pub fn downloading_pieces(&self) -> impl Iterator<Item = (PieceID, &Piece)> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.state == State::Downloading)
            .map(|(i, p)| (i as PieceID, p))
    }

    /// Returns an iterator of all pieces with state [`State::Pending`] or [`State::Downloading`]
    pub fn active_pieces(&self) -> impl Iterator<Item = (PieceID, &Piece)> {
        self.pending_pieces().chain(self.downloading_pieces())
    }

    /// Calculate the number of completed pieces (piece.state >= State::Verified)
    pub fn complete_pieces(&self) -> impl Iterator<Item = (PieceID, &Piece)> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.state >= State::Verified)
            .map(|(i, p)| (i as PieceID, p))
    }

    // note: maybe put in util & generic instead
    pub fn pieces_grouped(&self) -> BTreeMap<Priority, Vec<(usize, &Piece)>> {
        util::group_by_key(self.pieces.iter().enumerate(), |(_, piece)| piece.priority)
    }

    pub fn is_done(&self) -> bool {
        self.pieces.iter().all(|piece| piece.state == State::Done)
    }
}
