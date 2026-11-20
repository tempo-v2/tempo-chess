//! Read-only occurrences from a validated orthodox mainline.

use crate::standard_position_canonicalizer::board_key;
use crate::standard_uci::legal_move;
use crate::{ChessAnswer, LearnerColor, PositionKey};
use cozy_chess::Board;

/// The boards before, between and after the played moves, in game order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayedGameTimeline {
    occurrences: Vec<PositionOccurrence>,
}

impl PlayedGameTimeline {
    /// Offset zero is the starting board. The final board has no next move.
    pub fn occurrences(&self) -> &[PositionOccurrence] {
        &self.occurrences
    }

    pub(crate) fn from_mainline<'a>(
        mut board: Board,
        moves: impl ExactSizeIterator<Item = &'a str>,
    ) -> Self {
        let mut occurrences = Vec::with_capacity(moves.len() + 1);
        for uci in moves {
            occurrences.push(PositionOccurrence::new(
                occurrences.len(),
                &board,
                Some(ChessAnswer::from(uci)),
            ));
            let mv = legal_move(&board, uci).expect("played mainline contains only legal moves");
            board.play_unchecked(mv);
        }
        occurrences.push(PositionOccurrence::new(occurrences.len(), &board, None));
        Self { occurrences }
    }
}

/// One exact board occurrence. Equal canonical positions at different offsets
/// still describe distinct moments in the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionOccurrence {
    offset: usize,
    pub(crate) board: Board,
    fen: String,
    position: PositionKey,
    side_to_move: LearnerColor,
    next_move: Option<ChessAnswer>,
}

impl PositionOccurrence {
    fn new(offset: usize, board: &Board, next_move: Option<ChessAnswer>) -> Self {
        Self {
            offset,
            board: board.clone(),
            fen: board.to_string(),
            position: board_key(board),
            side_to_move: LearnerColor::from_board(board.side_to_move()),
            next_move,
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Full FEN from orthodox replay, retaining the parsed starting counters.
    /// Replay counters saturate at 100 halfmoves and u16::MAX fullmoves.
    /// The occurrence sequence retains the complete history independently.
    pub fn fen(&self) -> &str {
        &self.fen
    }

    pub fn position(&self) -> &PositionKey {
        &self.position
    }

    pub fn side_to_move(&self) -> LearnerColor {
        self.side_to_move
    }

    /// The move actually played from this board, in standard UCI.
    pub fn next_move(&self) -> Option<&ChessAnswer> {
        self.next_move.as_ref()
    }
}
