#[derive(Debug, Clone)]
pub struct Block {
	/// State of the block
	pub state: State,
	/// First byte of the block
	pub begin: u32,
	/// Size of the block in bytes
	pub size: u32
}

/// The state of a block
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
	Pending,
	Downloading,
	Done(Vec<u8>)
}
