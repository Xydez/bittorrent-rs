use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// How peer workers should prioritize a piece
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
	Highest
}

/// The state of a piece
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
	Done
}
