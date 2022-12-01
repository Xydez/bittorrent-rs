use crate::core::{
	block::{
		self,
		Block
	},
	configuration::Configuration
};

#[derive(Debug, Clone)]
pub struct PieceDownload {
	/// Status of the download
	pub blocks: Vec<Block>
}

// TODO: Maybe rename to something cooler, like Job or something
impl PieceDownload {
	pub fn new(config: &Configuration, piece_size: usize) -> PieceDownload {
		let blocks = (0..piece_size)
			.step_by(config.block_size as usize)
			.map(|i| Block {
				state: block::State::Pending,
				begin: i as u32,
				size: (piece_size as u32 - i as u32).min(config.block_size)
			})
			.collect::<Vec<_>>();

		log::debug!("PieceDownload created with {} blocks", blocks.len());

		PieceDownload { blocks }
	}

	pub fn data(&self) -> Option<Vec<u8>> {
		if !self.is_done() {
			None
		} else {
			self.blocks
				.iter()
				.map(|block| match &block.state {
					block::State::Done(data) => Some(data.clone()),
					_ => None
				})
				.collect::<Option<Vec<_>>>()
				.map(|vec| vec.concat())
		}
	}

	pub fn blocks(&self) -> impl Iterator<Item = &Block> {
		self.blocks.iter()
	}

	/// Returns true if all blocks of the piece download have a block state of [`BlockState::Done`]
	pub fn is_done(&self) -> bool {
		self.blocks
			.iter()
			.all(|block| matches!(block.state, block::State::Done(_)))
	}

	/// Returns true if the piece download has any blocks with a block state of [`BlockState::Pending`]
	pub fn has_pending(&self) -> bool {
		self.blocks
			.iter()
			.any(|block| block.state == block::State::Pending)
	}
}
