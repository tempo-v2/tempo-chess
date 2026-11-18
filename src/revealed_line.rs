//! Legal line positions for presentation of already disclosed moves.
use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;
use crate::{ChessDefinition, ChessError};
use cozy_chess::{Color, util::display_san_move};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealedLine {
    pub start_fen: String,
    pub moves: Vec<RevealedMove>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealedMove {
    pub notation: String,
    pub fen: String,
}

pub(super) fn numbered_san(board: &cozy_chess::Board, san: &str) -> String {
    let dots = if board.side_to_move() == Color::White {
        "."
    } else {
        "..."
    };
    format!("{}{dots} {san}", board.fullmove_number())
}

/// Project only the played prefix from an issued definition and result cursor.
/// For explicit content inspection the caller may authorize the full length.
pub fn revealed_line(
    definition: &ChessDefinition,
    played_decisions: usize,
) -> Result<RevealedLine, ChessError> {
    let invalid = || ChessError::InvalidContent("invalid revealed line cursor".into());
    let decisions = definition
        .decisions
        .get(..played_decisions)
        .ok_or_else(invalid)?;
    let first = definition.decisions.first().ok_or_else(invalid)?;
    let mut moves = Vec::new();
    for decision in decisions {
        let mut board = parse_position(&decision.fen, true)?;
        for reply in std::iter::once(&decision.expected).chain(&decision.autoplay) {
            let mv = legal_move(&board, &reply.uci)?;
            let notation = numbered_san(&board, &display_san_move(&board, mv).to_string());
            board.play(mv);
            moves.push(RevealedMove {
                notation,
                fen: board.to_string(),
            });
        }
    }
    Ok(RevealedLine {
        start_fen: first.fen.clone(),
        moves,
    })
}
