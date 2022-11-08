use super::session::{BLOCK_SIZE, PieceID};

#[derive(Debug, Clone)]
pub struct Block {
    pub state: BlockState,
    /// First byte of the block
    pub begin: u32,
    /// Size of the block in bytes
    pub size: u32
}

#[derive(Debug, Clone)]
pub struct PieceDownload {
    /// Piece index of the download
    pub piece: PieceID,
    /// Status 
    pub blocks: Vec<Block>
}

impl PieceDownload {
    pub fn new(piece: PieceID, piece_size: usize) -> PieceDownload {
        let blocks = (0..piece_size)
            .step_by(BLOCK_SIZE as usize)
            .map(|i| Block {
                state: BlockState::Pending,
                begin: i as u32,
                size: (piece_size as u32 - i as u32).min(BLOCK_SIZE)
            })
            .collect::<Vec<_>>();

        PieceDownload {
            piece,
            blocks
        }
    }

    /// Returns true if all blocks of the piece download have a block state of [`BlockState::Done`]
    pub fn is_done(&self) -> bool {
        self.blocks.iter().all(|block| matches!(block.state, BlockState::Done(_)))
    }

    /// Returns true if the piece download has any blocks with a block state of [`BlockState::Pending`]
    pub fn has_pending(&self) -> bool {
        self.blocks.iter().any(|block| block.state == BlockState::Pending)
    }

    // TODO: Utility function to get a pending block
}

#[derive(Debug, Clone)]
pub struct Piece {
    pub priority: Priority,
    pub state: State,
    pub availability: usize
}

impl Default for Piece {
    fn default() -> Self {
        Piece {
            priority: Priority::Normal,
            state: State::Pending,
            availability: 0
        }
    }
}

/// The state of a block
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockState {
    Pending,
    Downloading,
    Done(Vec<u8>) // TODO: Not all blocks are BLOCK_SIZE, maybe use Vec instead
}
/// How peer workers should prioritize a piece
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// All other pieces MUST have the state `State::Done` before pieces with priority `Priority::Lowest` MAY start downloading
    Lowest,
    /// Pieces with priority `Priority::Low` are selected after pieces with priority `Priority::Normal` or higher
    Low,
    /// Pieces start with `Priority::Normal` as the default and are thereafter selected according to the current selection algorithm
    Normal,
    /// Pieces with priority `Priority::High` are selected before pieces with priority `Priority::Normal` or lower
    High,
    /// All pieces with priority `Priority::Highest` MUST be `State::Verifying` or `State::Done` before any other pieces begin downloading
    Highest,
}

/// The state of a piece
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    /// The piece is ignored and should not be downloaded
    Ignore,
    /// The piece is marked for download and should be selected by a peer worker
    Pending,
    /// The piece has been selected by a peer worker and is being downloaded
    Downloading,
    /// The piece has been downloaded and should be selected by a verification worker
    Downloaded,
    /// The piece has been selected by a verification worker and is being verified
    Verifying,
    /// The piece has been verified and should be written to the store
    Verified,
    /// The piece has been completed and written to the store
    Done,
}
