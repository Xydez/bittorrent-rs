//! Piece selection strategies

// 3.2.5
// https://www.researchgate.net/publication/223808116_Implementation_and_analysis_of_the_BitTorrent_protocol_with_a_multi-agent_model
// https://www.researchgate.net/figure/Implementation-of-the-choking-algorithm_fig3_223808116

use rand::seq::{IteratorRandom, SliceRandom};
use tap::Tap;

use crate::core::{
	block, peer::Peer, piece, piece_download::PieceDownload, session::PieceId, torrent::Torrent
};

#[derive(Debug)]
pub struct Picker {
	pub end_game: bool
}

impl Picker {
	pub fn new() -> Self {
		Picker {
			end_game: true // TODO: Having end_game permanently enabled seems weird, either remove the variable completely or find some condition for enabling it
		}
	}

	pub fn select_piece(&self, torrent: &Torrent, peer: &Peer) -> Option<PieceId> {
		if let Some(i) = torrent
			.active_pieces()
			.filter(|(i, piece)| piece.priority == piece::Priority::Highest && peer.has_piece(*i))
			.choose(&mut rand::thread_rng())
			.map(|(i, _)| i)
		{
			// If there is any piece of priority Highest with state Pending or Downloading, download it
			Some(i)
		} else if torrent.complete_pieces().count() < 4 {
			// If we have less than four complete pieces, use a "random piece first" algorithm
			torrent
				.pending_pieces()
				.filter(|(i, _)| peer.has_piece(*i))
				.choose(&mut rand::thread_rng())
				.map(|(i, _)| i)
		} else {
			// If we have at least four complete pieces, use a "rarest piece first" so we will have more unusual pieces, which will be helpful in the trade with other peers.
			torrent.pending_pieces()
				.filter(|(i, _)| peer.has_piece(*i))
				.collect::<Vec<_>>()
				.tap_mut(|pieces|
					pieces.sort_by(|(_, a), (_, b)|
						a.priority
							.cmp(&b.priority) // TODO: Priority should be descending, availability should be ascending
							.then(a.availability.cmp(&b.availability))
					)
				)
				.into_iter()
				.take(8)
				.map(|(i, _)| i)
				.enumerate()
				.collect::<Vec<_>>()
				//.first()
				.choose_weighted(&mut rand::thread_rng(), |(i, _)| 256 / 2i32.pow(*i as u32 + 1))
				.ok()
				.map(|(_, p)| *p)
				.or_else(||
					// If we are in the end game, we are allowed to select downloading pieces if no pending pieces are found
					if self.end_game {
						torrent.downloading_pieces()
							.filter(|(i, _)| peer.has_piece(*i))
							.collect::<Vec<_>>()
							.tap_mut(|pieces|
								pieces.sort_by_key(|(_, piece)| piece.availability)
							)
							.choose(&mut rand::thread_rng())
							.map(|(p, _)| *p)
					} else {
						None
					}
				)
		}
		/*
		else {
			torrent.pending_pieces()
				.filter(|(i, _)| peer.has_piece(*i))
				.collect::<Vec<_>>()
				.tap_mut(|pieces|
					pieces.sort_by_key(|(_, piece)| piece.availability)
				)
				.choose(&mut rand::thread_rng())
				.map(|(i, _)| *i)
		}
		*/
	}

	pub fn select_block(&self, download: &PieceDownload) -> Option<usize> {
		// Returns the first pending block. Returns downloading blocks in endgame if no pending blocks are.
		download
			.blocks
			.iter()
			.enumerate()
			.find(|(_, block)| block.state == block::State::Pending)
			.or_else(|| {
				if
				/*self.end_game*/
				false {
					// TODO: Get end_game to work correctly
					// TODO: We need to make sure we aren't already requesting the block from the peer
					download
						.blocks
						.iter()
						.enumerate()
						.filter(|(_, block)| block.state == block::State::Downloading)
						.choose(&mut rand::thread_rng())
				} else {
					None
				}
			})
			.map(|(i, _)| i)
	}
}

impl Default for Picker {
	fn default() -> Self {
		Self::new()
	}
}
