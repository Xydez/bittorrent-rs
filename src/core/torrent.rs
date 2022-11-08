use std::{sync::Arc, collections::{BTreeMap, HashMap}};

use tokio::sync::Mutex;

use crate::{protocol::{metainfo::MetaInfo, tracker::Tracker}, io::store::Store};

use super::{piece::{Piece, Priority, State, PieceDownload}, util, peer_worker::Worker};

#[derive(Debug)]
pub struct Torrent {
    pub meta_info: MetaInfo,
    pub peers: Vec<Worker>,
    pub store: Arc<Mutex<Box<dyn Store>>>,
    pub pieces: Vec<Piece>,
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
            store: Arc::new(Mutex::new(store)),
            downloads: HashMap::new(),
            tracker
        }
    }

    /// Calculate the number of completed pieces (piece.state >= State::Verified)
    pub fn completed_pieces(&self) -> usize {
        self.pieces
            .iter()
            .filter(|piece| piece.state >= State::Verified)
            .count()
    }

    // note: maybe put in util & generic instead
    pub fn pieces_grouped(&self) -> BTreeMap<Priority, Vec<(usize, &Piece)>> {
        util::group_by_key(self.pieces.iter().enumerate(), |(_, piece)| piece.priority)
    }

    /// Enter endgame if all pieces are currently downloading
    pub fn is_endgame(&self) -> bool {
        !self.pieces.iter().any(|piece| piece.state == State::Pending)
    }
}
