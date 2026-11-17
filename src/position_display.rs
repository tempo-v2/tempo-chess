//! Presentation of the current board from an issued definition and saved cursor.

use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;
use crate::{ChessDefinition, ChessError, ChessState, LearnerColor};
use cozy_chess::Color;
use cozy_chess::util::display_uci_move;

/// Board facts safe to send to a learner. The move list is empty when locked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionDisplay {
    pub display_fen: String,
    pub learner_color: LearnerColor,
    pub legal_moves: Vec<String>,
    pub in_check: bool,
}

/// Project a ready, completed or abandoned attempt using the definition version
/// it was issued. Completion reconstructs the final board, including autoplay.
pub fn project_position(
    definition: &ChessDefinition,
    state: &ChessState,
    ready: bool,
) -> Result<PositionDisplay, ChessError> {
    let decisions = &definition.decisions;
    let invalid = || ChessError::InvalidContent("invalid board projection cursor".into());
    let first = decisions.first().ok_or_else(invalid)?;
    let index = state.decision_index;
    let learner_color = match parse_position(&first.fen, true)?.side_to_move() {
        Color::White => LearnerColor::White,
        Color::Black => LearnerColor::Black,
    };
    let board = match decisions.get(index) {
        Some(decision) => parse_position(&decision.fen, true)?,
        None if !ready && index == decisions.len() => {
            let last = decisions.last().ok_or_else(invalid)?;
            let mut board = parse_position(&last.fen, true)?;
            for answer in std::iter::once(&last.expected).chain(&last.autoplay) {
                let mv = legal_move(&board, &answer.uci)?;
                board.play(mv);
            }
            board
        }
        None => return Err(invalid()),
    };
    let mut legal_moves = Vec::new();
    if ready {
        board.generate_moves(|moves| {
            legal_moves.extend(
                moves
                    .into_iter()
                    .map(|mv| display_uci_move(&board, mv).to_string()),
            );
            false
        });
        legal_moves.sort_unstable();
    }
    // The stored decision FEN is also the prompt for a ready attempt.
    let display_fen = decisions
        .get(index)
        .map_or_else(|| board.to_string(), |d| d.fen.clone());
    Ok(PositionDisplay {
        display_fen,
        learner_color,
        legal_moves,
        in_check: !board.checkers().is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TacticRecord, compile_tactics};

    #[test]
    fn projects_all_promotions_castling_en_passant_and_the_final_reply() {
        // Synthetic positions exercise special-move notation and legality.
        for (fen, solution, choices, last_fen) in [
            (
                "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                ["a7a8n", "e8e7"],
                vec!["a7a8b", "a7a8n", "a7a8q", "a7a8r"],
                "N7/4k3/8/8/8/8/8/4K3 w - - 1 2",
            ),
            (
                "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
                ["e1g1", "e8c8"],
                vec!["e1g1", "e1c1"],
                "2kr3r/8/8/8/8/8/8/R4RK1 w - - 2 2",
            ),
            (
                "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2",
                ["e5d6", "e8f7"],
                vec!["e5d6"],
                "8/5k2/3P4/8/8/8/8/4K3 w - - 1 2",
            ),
        ] {
            let row = TacticRecord {
                namespace: "synthetic".into(),
                source_id: "position-display".into(),
                solver_fen: fen.into(),
                solution: solution.map(String::from).to_vec(),
                label: "Synthetic special move".into(),
                provenance: vec![],
            };
            let catalog = compile_tactics(&[row]).unwrap();
            let definition = &catalog.items.values().next().unwrap().definition;
            let ready =
                project_position(definition, &ChessState { decision_index: 0 }, true).unwrap();
            for uci in choices {
                assert!(ready.legal_moves.contains(&uci.to_owned()));
            }
            assert!(!ready.legal_moves.contains(&"e1h1".into()));
            let abandoned =
                project_position(definition, &ChessState { decision_index: 0 }, false).unwrap();
            assert_eq!(abandoned.display_fen, ready.display_fen);
            assert!(abandoned.legal_moves.is_empty());
            let completed =
                project_position(definition, &ChessState { decision_index: 1 }, false).unwrap();
            assert_eq!(completed.display_fen, last_fen);
            assert_eq!(completed.learner_color, LearnerColor::White);
            assert!(completed.legal_moves.is_empty());
        }
    }

    #[test]
    fn a_black_learner_stays_black_when_the_completed_position_checks_white() {
        let row = TacticRecord {
            namespace: "synthetic".into(),
            source_id: "black".into(),
            solver_fen: "4k3/4r3/8/8/8/8/8/5K2 b - - 0 1".into(),
            solution: vec!["e7f7".into()],
            label: "Synthetic black move".into(),
            provenance: vec![],
        };
        let catalog = compile_tactics(&[row]).unwrap();
        let definition = &catalog.items.values().next().unwrap().definition;
        let completed =
            project_position(definition, &ChessState { decision_index: 1 }, false).unwrap();
        assert_eq!(completed.learner_color, LearnerColor::Black);
        assert!(completed.in_check);
        assert!(completed.display_fen.contains(" w "));
    }
}
