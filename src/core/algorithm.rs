//! Piece selection strategies

// 3.2.5
// https://www.researchgate.net/publication/223808116_Implementation_and_analysis_of_the_BitTorrent_protocol_with_a_multi-agent_model
// https://www.researchgate.net/figure/Implementation-of-the-choking-algorithm_fig3_223808116

use rand::seq::{SliceRandom, IteratorRandom};
use tap::Tap;

use super::{piece, session::PieceID};

/// Select a piece to download using the standard selection algorithm
///
/// 1. Download all pieces with Priority::Highest
/// 2. Use a "random piece first" algorithm until we have four complete pieces
/// 2. Download all pieces in order of Priority::High, Priority::Normal and Priority::Low
/// 3. Download all pieces with Priority::Lowest
pub fn select_piece(pieces: &[(PieceID, &piece::Piece)], end_game: bool) -> Option<PieceID> {
    // Pieces with state == Pending || state == Downloading
    let active_pieces = pieces.iter()
        .filter(|(_, piece)| piece.state == piece::State::Pending || piece.state == piece::State::Downloading)
        .collect::<Vec<_>>();

    let pending_pieces = active_pieces.iter()
        .copied()
        .filter(|(_, piece)| piece.state == piece::State::Pending)
        .collect::<Vec<_>>();
    
    let complete_pieces = pieces.iter()
        .filter(|(_, piece)| piece.state >= piece::State::Verified)
        .count();

    // TODO: End game

    if let Some(i) = active_pieces.iter()
        .filter(|(_, piece)|
            piece.priority == piece::Priority::Highest
        )
        .choose(&mut rand::thread_rng())
        .map(|(i, _)| *i) {
        // If there is any piece of priority Highest with state Pending or Downloading, download it
        Some(i)
    } else if complete_pieces < 4 {
        // If we have less than four complete pieces, use a "random piece first" algorithm
        if let Some(i) = pending_pieces
            .choose(&mut rand::thread_rng())
            .map(|(i, _)| *i) {
            Some(i)
        } else {
            None
        }
    } else if !end_game {
        // If we have at least four complete pieces, use a "rarest piece first" so we will have more unusual pieces, which will be helpful in the trade with other peers.
        if let Ok(i) = pending_pieces.clone()
            .tap_mut(|pieces|
                pieces.sort_by(|(_, a), (_, b)|
                    a.priority
                        .cmp(&b.priority)
                        .then(a.availability.cmp(&b.availability))
                )
            )
            .into_iter()
            .take(8)
            .map(|(i, _)| i)
            .enumerate()
            .collect::<Vec<_>>()
            .choose_weighted(&mut rand::thread_rng(), |(i, _)| 256 / 2i32.pow(*i as u32 + 1))
            .map(|(_, p)| **p) {
            Some(i)
        } else {
            None
        }
    } else {
        // If we are in the end game, select a random piece with state Pending or Downloading
        active_pieces.clone()
            .tap_mut(|pieces|
                pieces.sort_by_key(|(_, piece)| piece.availability)
            )
            .choose(&mut rand::thread_rng())
            .map(|(i, _)| *i)
    }
}
