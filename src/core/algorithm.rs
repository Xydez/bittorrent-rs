//! Piece selection strategies

// 3.2.5
// https://www.researchgate.net/publication/223808116_Implementation_and_analysis_of_the_BitTorrent_protocol_with_a_multi-agent_model
// https://www.researchgate.net/figure/Implementation-of-the-choking-algorithm_fig3_223808116

use rand::seq::{SliceRandom, IteratorRandom};
use tap::Tap;

use super::{block, piece, session::PieceID, torrent::Torrent, peer::Peer, piece_download::PieceDownload};

#[derive(Debug)]
pub struct Picker {
    pub end_game: bool
}

impl Picker {
    pub fn new() -> Self {
        Picker {
            end_game: false
        }
    }

    pub fn select_piece(&self, torrent: &Torrent, peer: &Peer) -> Option<PieceID> {
        if let Some(i) = torrent.active_pieces()
            .filter(|(i, piece)|
                piece.priority == piece::Priority::Highest && peer.has_piece(*i)
            )
            .choose(&mut rand::thread_rng())
            .map(|(i, _)| i) {
            // If there is any piece of priority Highest with state Pending or Downloading, download it
            Some(i)
        } else if torrent.complete_pieces().count() < 4 {
            // If we have less than four complete pieces, use a "random piece first" algorithm
            torrent.pending_pieces()
                .filter(|(i, _)| peer.has_piece(*i))
                .choose(&mut rand::thread_rng())
                .map(|(i, _)| i)
        } else if !self.end_game {
            // If we have at least four complete pieces, use a "rarest piece first" so we will have more unusual pieces, which will be helpful in the trade with other peers.
            torrent.pending_pieces()
                .filter(|(i, _)| peer.has_piece(*i))
                .collect::<Vec<_>>()
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
                .ok()
                .map(|(_, p)| *p)
                .or_else(||
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
        } else {
            // If we are in the end game, select a random piece with state Pending, or Downloading if no pending pieces are found
            torrent.pending_pieces()
                .filter(|(i, _)| peer.has_piece(*i))
                .collect::<Vec<_>>()
                .tap_mut(|pieces|
                    pieces.sort_by_key(|(_, piece)| piece.availability)
                )
                .choose(&mut rand::thread_rng())
                .map(|(i, _)| *i)
        }
    }

    pub fn select_block(&self, download: &PieceDownload) -> Option<usize> { // Option<(u32, u32)>
        // Returns the first pending block. Returns downloading blocks in endgame if no pending blocks are.
        download.blocks
            .iter()
            .enumerate()
            .find(|(_, block)| block.state == block::State::Pending)
            .or_else(||
                if self.end_game {
                    download.blocks
                        .iter()
                        .enumerate()
                        .find(|(_, block)| block.state == block::State::Downloading)
                } else {
                    None
                }
            )
            .map(|(i, _)| i)
    }
}

impl Default for Picker {
    fn default() -> Self {
        Self::new()
    }
}
