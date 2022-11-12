use std::{sync::Arc, collections::{BTreeMap, HashMap}};

use tokio::sync::{Mutex, RwLock};

use crate::{protocol::{metainfo::MetaInfo, tracker::Tracker}, io::store::Store};

use super::{piece::{Piece, Priority, State}, util, worker::Worker, algorithm::Picker, session::{PieceID, BLOCK_SIZE}, block::{Block, self}};

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
    pub picker: Arc<RwLock<Picker>>,
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
            picker: Arc::new(RwLock::new(Picker { end_game: false })),
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

    /// Returns an iterator of all pieces with state [`State::Pending`] or [`State::Downloading`]
    pub fn active_pieces(&self) -> impl Iterator<Item = (PieceID, &Piece)> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.state == State::Pending || piece.state == State::Downloading)
            .map(|(i, p)| (i as PieceID, p))
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
}

#[derive(Debug, Clone)]
pub struct PieceDownload {
    /// Status of the download
    pub blocks: Vec<Block>
}

// TODO: Maybe rename to something cooler, like Job or something
impl PieceDownload {
    pub fn new(piece_size: usize) -> PieceDownload {
        let blocks = (0..piece_size)
            .step_by(BLOCK_SIZE as usize)
            .map(|i| Block {
                state: block::State::Pending,
                begin: i as u32,
                size: (piece_size as u32 - i as u32).min(BLOCK_SIZE)
            })
            .collect::<Vec<_>>();

        PieceDownload {
            blocks
        }
    }

    pub fn data(&self) -> Option<Vec<u8>> {
        self.blocks
            .iter()
            .map(|block| match &block.state {
                block::State::Done(data) => Some(data.clone()),
                _ => None
            })
            .collect::<Option<Vec<_>>>()
            .map(|vec| vec.concat())
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
    }

    /// Returns true if all blocks of the piece download have a block state of [`BlockState::Done`]
    pub fn is_done(&self) -> bool {
        self.blocks.iter().all(|block| matches!(block.state, block::State::Done(_)))
    }

    /// Returns true if the piece download has any blocks with a block state of [`BlockState::Pending`]
    pub fn has_pending(&self) -> bool {
        self.blocks.iter().any(|block| block.state == block::State::Pending)
    }
}
