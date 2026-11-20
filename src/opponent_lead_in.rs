//! Verified presentation context for the single ply before a learner decision.
use crate::parsed_pgn::PgnGame;
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::standard_uci::legal_move;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpponentLeadIn {
    pub before_fen: String,
    pub uci: String,
}

impl OpponentLeadIn {
    /// Accept only a legal move reaching the solver position, ignoring clocks
    /// and ineffective en passant just as the content compilers do.
    pub fn verified(before_fen: &str, uci: &str, solver_fen: &str) -> Option<Self> {
        let mut board = parse_position(before_fen, true).ok()?;
        let before_fen = format!("{} 0 1", board_key(&board).0);
        let mv = legal_move(&board, uci).ok()?;
        board.play(mv);
        let solver = parse_position(solver_fen, true).ok()?;
        (board_key(&board) == board_key(&solver)).then(|| Self {
            before_fen,
            uci: uci.into(),
        })
    }

    pub(super) fn from_pgn(game: &PgnGame<'_>, solver_index: usize) -> Option<Self> {
        let prior = &game.plies[game.plies[solver_index].previous?];
        Some(Self {
            before_fen: format!("{} 0 1", board_key(&prior.board).0),
            uci: prior.uci.clone(),
        })
    }
}

/// Recover candidate setup moves from an archived PGN associated with a tactic.
/// A selected range can start anywhere on a branch, so match the whole saved
/// solution before reporting a preceding move. The caller resolves conflicts
/// across all associated sources.
pub fn archived_tactic_lead_ins(
    pgn: &str,
    definition: &crate::ChessDefinition,
) -> Result<Vec<OpponentLeadIn>, crate::ChessError> {
    let crate::ContentKind::Tactic { practice_start } = &definition.kind else {
        return Ok(Vec::new());
    };
    let solution: Vec<_> = definition
        .decisions
        .iter()
        .flat_map(|decision| {
            std::iter::once(&decision.expected)
                .chain(&decision.autoplay)
                .map(|m| m.uci.as_str())
        })
        .collect();
    if solution.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for game in crate::parsed_pgn::parse_pgn_records(pgn)?
        .into_iter()
        .flatten()
    {
        for route in game.routes() {
            for segment in route.windows(solution.len()) {
                let index = segment[0];
                if &board_key(&game.plies[index].board) == practice_start
                    && segment
                        .iter()
                        .zip(&solution)
                        .all(|(&i, uci)| game.plies[i].uci == *uci)
                    && let Some(context) = OpponentLeadIn::from_pgn(&game, index)
                    && !matches.contains(&context)
                {
                    matches.push(context);
                }
            }
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LearnerColor, compile_repertoire, preview_tactic_pgn};

    #[test]
    fn lead_in_comes_only_from_a_real_predecessor() {
        let authored = compile_repertoire(
            "1. e4 c5 2. Nf3 *",
            "white",
            "Book",
            LearnerColor::White,
            "",
        )
        .unwrap();
        let item = |uci| {
            authored
                .items
                .values()
                .find(|item| item.definition.decisions[0].expected.uci == uci)
                .unwrap()
        };
        assert_eq!(item("e2e4").opponent_lead_in, None);
        let response = item("g1f3");
        let context = response.opponent_lead_in.as_ref().unwrap();
        assert_eq!(context.uci, "c7c5");
        let solver = &response.definition.decisions[0].fen;
        assert_eq!(
            OpponentLeadIn::verified(&context.before_fen, &context.uci, solver).as_ref(),
            Some(context)
        );
        assert_eq!(
            OpponentLeadIn::verified(&context.before_fen, "e7e5", solver),
            None
        );
    }

    #[test]
    fn archived_capture_requires_the_complete_solution() {
        let pgn = format!(
            "[SetUp \"1\"]\n[FEN \"{}\"]\n[TempoSetupPlies \"1\"]\n\n1. e4 c5 2. Nf3 d6 *",
            crate::START_FEN
        );
        let preview = preview_tactic_pgn(&pgn, &[], "").unwrap();
        let catalog = preview.selected_catalog(&[0]).unwrap();
        let item = catalog.items.values().next().unwrap();
        assert_eq!(item.opponent_lead_in.as_ref().unwrap().uci, "e2e4");
        assert_eq!(
            archived_tactic_lead_ins(&pgn, &item.definition).unwrap(),
            item.opponent_lead_in
                .clone()
                .into_iter()
                .collect::<Vec<_>>()
        );
        let changed = pgn.replace("Nf3 d6", "Nf3 Nc6");
        assert!(
            archived_tactic_lead_ins(&changed, &item.definition)
                .unwrap()
                .is_empty()
        );
    }
}
