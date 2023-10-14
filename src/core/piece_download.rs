use std::{collections::VecDeque, sync::Arc};

use log::debug;

use crate::{core::configuration::Configuration, torrent::WorkerId};

#[derive(Debug, Clone)]
pub struct Block {
	/// First byte of the block
	pub begin: u32,
	/// Size of the block in bytes
	pub size: u32,
	/// Data of the block
	pub data: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone)]
pub struct PieceDownload {
	/// Queue of (WorkerId, BlockId)
	pub block_downloads: VecDeque<(u32, WorkerId)>,
	/// Status of the download
	pub blocks: Vec<Block>,
}

impl PieceDownload {
	pub fn new(config: &Configuration, piece_size: usize) -> PieceDownload {
		let blocks = (0..piece_size)
			.step_by(config.block_size)
			.map(|i| Block {
				begin: i as u32,
				size: (piece_size - i).min(config.block_size) as u32,
				data: None,
			})
			.collect::<Vec<_>>();

		debug!("PieceDownload created with {} blocks", blocks.len());

		PieceDownload {
			blocks,
			block_downloads: VecDeque::new(),
		}
	}

	pub fn data(&self) -> Option<Vec<u8>> {
		self.blocks
			.iter()
			.map(|block| block.data.as_ref())
			.collect::<Option<Vec<_>>>() // Vec<&Arc<[u8]>>
			.map(|vec| {
				vec.iter().fold(Vec::new(), |mut acc, vec| {
					acc.extend_from_slice(vec);
					acc
				})
			})
	}

	pub fn blocks(&self) -> impl Iterator<Item = &Block> {
		self.blocks.iter()
	}

	pub fn pending_blocks(&self) -> impl Iterator<Item = (u32, &Block)> {
		self.blocks()
			.enumerate()
			.map(|(block_id, block)| (block_id as u32, block))
			.filter(|(block_id, block)| {
				block.data.is_none()
					&& self
						.block_downloads
						.binary_search_by_key(&block_id, |(block_id, _)| block_id)
						.is_err()
			})
	}

	/// Returns true if all blocks of the piece download have a block state of [`BlockState::Done`]
	pub fn is_done(&self) -> bool {
		self.blocks.iter().all(|block| block.data.is_some())
	}
}
