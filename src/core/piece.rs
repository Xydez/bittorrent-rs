#[derive(Debug, Clone)]
pub struct Piece {
    pub priority: Priority,
    pub state: State,
}

/// How peer workers should prioritize downloading the piece. Pieces are selected using the current selecting algorithm
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl State {
    fn to_i(&self) -> usize {
        match self {
            State::Ignore => 0,
            State::Pending => 1,
            State::Downloading => 2,
            State::Downloaded => 3,
            State::Verifying => 4,
            State::Verified => 5,
            State::Done => 6,
        }
    }
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_i().cmp(&other.to_i())
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
