//! Legal line positions for presentation of already disclosed moves.
use crate::parsed_pgn::parse_pgn;
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::standard_uci::legal_move;
use crate::{ChessDefinition, ChessError, PositionKey};
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

/// Return the first full authored route in PGN order containing the issued
/// canonical position and its prescribed move. The caller authorizes disclosure.
pub fn first_matching_repertoire_line(
    pgn: &str,
    position: &PositionKey,
    expected_uci: &str,
) -> Result<Option<RevealedLine>, ChessError> {
    for game in parse_pgn(pgn)? {
        let matches: Vec<_> = game
            .plies
            .iter()
            .map(|ply| ply.uci == expected_uci && &board_key(&ply.board) == position)
            .collect();
        if let Some(route) = game
            .routes()
            .find(|route| route.iter().any(|&index| matches[index]))
        {
            let start_fen = game.plies[route[0]].board.to_string();
            let moves = route
                .into_iter()
                .map(|index| {
                    let ply = &game.plies[index];
                    let mut after = ply.board.clone();
                    after.play(ply.mv);
                    RevealedMove {
                        notation: numbered_san(&ply.board, &ply.san),
                        fen: after.to_string(),
                    }
                })
                .collect();
            return Ok(Some(RevealedLine { start_fen, moves }));
        }
    }
    Ok(None)
}

/// Project only the played prefix from an issued definition and result cursor.
/// For explicit content inspection the caller may authorize the full length.
pub fn revealed_line(
    definition: &ChessDefinition,
    played_decisions: usize,
) -> Result<RevealedLine, ChessError> {
    revealed_attempt_line(definition, &crate::ChessState::at(played_decisions))
}

pub fn revealed_attempt_line(
    definition: &ChessDefinition,
    state: &crate::ChessState,
) -> Result<RevealedLine, ChessError> {
    let played_decisions = state.decision_index;
    let invalid = || ChessError::InvalidContent("invalid revealed line cursor".into());
    let decisions = definition
        .decisions
        .get(..played_decisions)
        .ok_or_else(invalid)?;
    let first = definition.decisions.first().ok_or_else(invalid)?;
    let mut moves = Vec::new();
    for (index, decision) in decisions.iter().enumerate() {
        let mut board = parse_position(&decision.fen, true)?;
        let played = state
            .played_mate
            .as_ref()
            .filter(|_| index + 1 == definition.decisions.len());
        for reply in std::iter::once(played.unwrap_or(&decision.expected)).chain(&decision.autoplay)
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_export_matches_the_issued_position_and_move_in_pgn_order() {
        let pgn = "1.e4 e5 (1...c5 2.Nf3 d6 (2...Nc6 3.Bb5) 3.d4) 2.Nf3 Nc6 *";
        let first = &parse_pgn(pgn).unwrap()[0];
        let position = board_key(
            &first
                .plies
                .iter()
                .find(|ply| ply.uci == "c7c5")
                .unwrap()
                .board,
        );
        let line = first_matching_repertoire_line(pgn, &position, "c7c5")
            .unwrap()
            .unwrap();
        assert_eq!(line.start_fen, cozy_chess::Board::default().to_string());
        assert_eq!(
            line.moves
                .iter()
                .map(|ply| ply.notation.as_str())
                .collect::<Vec<_>>(),
            ["1. e4", "1... c5", "2. Nf3", "2... d6", "3. d4"]
        );
        assert!(
            first_matching_repertoire_line(pgn, &position, "e7e6")
                .unwrap()
                .is_none()
        );
    }
}
