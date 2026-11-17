//! Prepare a solver-start FEN from an explicitly identified setup ply.

use crate::ChessError;
use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;

/// Apply the single opponent setup move supplied by a source converter.
/// Stored solver-first cards can go straight to [`crate::compile_tactics`].
pub fn apply_setup_move(fen: &str, uci: &str) -> Result<String, ChessError> {
    let mut board = parse_position(fen, true)?;
    let mv = legal_move(&board, uci)?;
    board.play(mv);
    Ok(board.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_only_the_explicit_pack_setup_and_changes_turn() {
        // First source row of legacy fork-focused-01 (Lichess puzzle 01gro).
        let fen = "r3k2r/ppp4p/3qpQp1/5n2/4R3/2P1P2P/P4PP1/5RK1 b kq - 9 22";
        assert_eq!(
            apply_setup_move(fen, "h8f8").unwrap(),
            "r3kr2/ppp4p/3qpQp1/5n2/4R3/2P1P2P/P4PP1/5RK1 w q - 10 23"
        );
        assert!(apply_setup_move(fen, "e4e6").is_err());
    }
}
