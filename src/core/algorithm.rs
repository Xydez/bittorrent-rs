//! Piece selection strategies

// 3.2.5
// https://www.researchgate.net/publication/223808116_Implementation_and_analysis_of_the_BitTorrent_protocol_with_a_multi-agent_model
// https://www.researchgate.net/figure/Implementation-of-the-choking-algorithm_fig3_223808116

use rand::seq::{IteratorRandom, SliceRandom};
use tap::Tap;

use super::{
	configuration::Configuration,
	piece::{Priority, State},
	piece_download::PieceDownload,
};
use crate::{
	peer::Peer,
	torrent::{TorrentLock, WorkerId},
};

// TODO: In all honesty we really don't want async hhere..
/// Selects a NEW piece according to the following priority rules
/// 1. Download all pieces with [`Priority::Highest`] in random order
/// 2. If there are less than 4 complete pieces, download a random piece
/// 3. Download pieces sorted by descending priority, then ascending availability, using a weighted probability distribution where `weight(x) = floor(256 / 2^x)`\*.
///
/// \* In practice this means choosing a random piece of the 8 most rare pieces.
pub fn select_piece(torrent: &TorrentLock<'_>, peer: &Peer, config: &Configuration) -> Option<u32> {
	// If there is any piece of priority Highest with state Pending, download it
	if let Some(i) = torrent
		.pieces()
		.filter_map(|(i, piece)| {
			(piece.priority == Priority::Highest
				&& matches!(piece.state, State::Pending)
				&& peer.has_piece(i))
			.then_some(i)
		})
		.choose(&mut rand::thread_rng())
	{
		return Some(i);
	}

	// If we have less than four complete pieces, use a "random piece first" algorithm
	if torrent
		.pieces()
		.filter(|(_, piece)| piece.state >= State::Writing)
		.count() < config.initial_pieces
	{
		return torrent
			.pieces()
			.filter_map(|(i, piece)| {
				(piece.state == State::Pending && peer.has_piece(i)).then_some(i)
			})
			.choose(&mut rand::thread_rng());
	}

	// If we have at least four complete pieces, use a "rarest piece first" so we will have more unusual pieces, which will be helpful in the trade with other peers.
	torrent
		.pieces()
		.filter(|(i, piece)| piece.state == State::Pending && peer.has_piece(*i))
		.collect::<Vec<_>>()
		.tap_mut(|pieces| {
			pieces.sort_by(|(a_id, a), (b_id, b)| {
				torrent
					.state()
					.downloads
					.contains_key(a_id)
					.cmp(&torrent.state().downloads.contains_key(b_id))
					.then(b.priority.cmp(&a.priority))
					.then(a.availability.cmp(&b.availability))
			})
		})
		.into_iter()
		.take(8)
		.map(|(i, _)| i)
		.enumerate()
		.collect::<Vec<_>>()
		.choose_weighted(&mut rand::thread_rng(), |(i, _)| {
			256 / 2i32.pow(*i as u32 + 1)
		})
		.ok()
		.map(|(_, p)| *p)
}

/// Selects blocks that the worker is not already downloading using the following priority rules
///
/// 1. Download all pending blocks in order
/// 2. If no pending blocks are found, download the block with the least amount of workers currently working on it
pub fn select_block(
	download: &PieceDownload,
	worker_id: WorkerId, //end_game: bool
) -> Option<u32> {
	download
		.blocks
		.iter()
		.enumerate()
		// Filter only pending blocks
		.filter(|(_, block)| block.data.is_none())
		// Filter only blocks not already being downloaded by the same worker id
		.filter(|(i, _)| {
			!download
				.block_downloads
				.iter()
				.any(|(block_id, download_worker_id)| {
					worker_id == *download_worker_id && *block_id == *i as u32
				})
		})
		.map(|(i, _)| {
			(
				i,
				download
					.block_downloads
					.iter()
					.filter(|(block_id, _)| *block_id == i as u32)
					.count(),
			)
		})
		//.filter(|(_, worker_count)| end_game || *worker_count == 0)
		.collect::<Vec<_>>()
		.tap_mut(|vec| {
			vec.sort_unstable_by(
				|(block_id_a, worker_count_a), (block_id_b, worker_count_b)| {
					worker_count_a
						.cmp(worker_count_b)
						.then(block_id_a.cmp(block_id_b))
				},
			)
		})
		.first()
		.map(|(i, _)| *i as u32)
}
