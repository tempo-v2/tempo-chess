//! Standard UCI at the boundary with the chess library.

use crate::ChessError;
use crate::chess_answer::valid_uci;
use cozy_chess::util::{display_uci_move, parse_uci_move};
use cozy_chess::{Board, Move};

pub(crate) fn legal_move(board: &Board, uci: &str) -> Result<Move, ChessError> {
    if !valid_uci(uci) {
        return Err(ChessError::InvalidContent(format!(
            "malformed UCI move {uci:?}"
        )));
    }
    let illegal = || ChessError::InvalidContent(format!("illegal move {uci:?}"));
    let mv = parse_uci_move(board, uci).map_err(|_| illegal())?;
    // The underlying move representation also accepts king-to-rook castling.
    // The wire and tactic data use the orthodox king-destination spelling.
    if !board.is_legal(mv) || display_uci_move(board, mv).to_string() != uci {
        return Err(illegal());
    }
    Ok(mv)
}
