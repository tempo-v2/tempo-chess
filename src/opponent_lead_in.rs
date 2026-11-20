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

/// Reconcile verified predecessors that describe the same solver decision.
/// An absent source adds no evidence. A disagreement survives subsequent merges
/// until the authored content is explicitly replaced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeadInEvidence {
    #[default]
    Unavailable,
    Verified(OpponentLeadIn),
    Conflicting,
}

impl LeadInEvidence {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }

    pub fn verified_move(&self) -> Option<&OpponentLeadIn> {
        match self {
            Self::Verified(lead_in) => Some(lead_in),
            _ => None,
        }
    }

    pub fn reconcile(&mut self, other: &Self) {
        match (&*self, other) {
            (_, Self::Unavailable) | (Self::Conflicting, _) => {}
            (Self::Unavailable, other) => self.clone_from(other),
            (current, other) if current == other => {}
            _ => *self = Self::Conflicting,
        }
    }
}

impl From<Option<OpponentLeadIn>> for LeadInEvidence {
    fn from(lead_in: Option<OpponentLeadIn>) -> Self {
        lead_in.map(Self::Verified).unwrap_or_default()
    }
}

/// Recover setup evidence from an archived PGN associated with a tactic.
/// A selected range can start anywhere on a branch, so match the whole saved
/// solution before combining its predecessors. The caller reconciles any
/// other associated sources for this definition.
pub fn archived_tactic_lead_in(
    pgn: &str,
    definition: &crate::ChessDefinition,
) -> Result<LeadInEvidence, crate::ChessError> {
    let crate::ContentKind::Tactic { practice_start } = &definition.kind else {
        return Ok(LeadInEvidence::Unavailable);
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
        return Ok(LeadInEvidence::Unavailable);
    }

    let mut evidence = LeadInEvidence::Unavailable;
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
                {
                    evidence.reconcile(&LeadInEvidence::Verified(context));
                }
            }
        }
    }
    Ok(evidence)
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
        assert_eq!(item("e2e4").opponent_lead_in, LeadInEvidence::Unavailable);
        let response = item("g1f3");
        let context = response.opponent_lead_in.verified_move().unwrap();
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
        assert_eq!(item.opponent_lead_in.verified_move().unwrap().uci, "e2e4");
        assert_eq!(
            archived_tactic_lead_in(&pgn, &item.definition).unwrap(),
            item.opponent_lead_in
        );
        let changed = pgn.replace("Nf3 d6", "Nf3 Nc6");
        assert!(
            archived_tactic_lead_in(&changed, &item.definition)
                .unwrap()
                .is_unavailable()
        );
    }
    #[test]
    fn transposed_repertoire_sources_reconcile_independently_of_order() {
        let first = "1. Nf3 d5 2. d4 Nf6 3. c4 *";
        let second = "1. Nf3 Nf6 2. d4 d5 3. c4 *";
        for pgn in [
            format!("{first}\n\n{second}"),
            format!("{second}\n\n{first}"),
        ] {
            let book = compile_repertoire(&pgn, "white", "Book", LearnerColor::White, "").unwrap();
            let target = book
                .items
                .values()
                .find(|item| item.definition.decisions[0].expected.uci == "c2c4")
                .unwrap();
            assert_eq!(target.opponent_lead_in, LeadInEvidence::Conflicting);
            assert_eq!(target.verified_opponent_lead_in(), None);
        }

        let repeated = compile_repertoire(
            "1. Nf3 Nf6 2. Ng1 Ng8 3. Nf3 *",
            "white",
            "Book",
            LearnerColor::White,
            "",
        )
        .unwrap();
        let target = repeated
            .items
            .values()
            .find(|item| item.definition.decisions[0].expected.uci == "g1f3")
            .unwrap();
        assert_eq!(target.verified_opponent_lead_in().unwrap().uci, "f6g8");
    }

    #[test]
    fn tactic_aliases_keep_conflicts_through_import_capture_and_a_later_known_source() {
        let source = |id, before, san| {
            let pgn = format!(
                "[SetUp \"1\"]\n[FEN \"{before}\"]\n[TempoSetupPlies \"1\"]\n[TempoSource \"fixture\"]\n[TempoSourceId \"{id}\"]\n\n1... {san} 2. e4 *"
            );
            preview_tactic_pgn(&pgn, &[], "")
                .unwrap()
                .selected_catalog(&[0])
                .unwrap()
        };
        let a = source("a", "4k1n1/8/8/8/8/8/4P3/4K3 b - - 0 1", "Nf6");
        let b = source("b", "4k3/8/8/7n/8/8/4P3/4K3 b - - 0 1", "Nf6");
        for (first, second) in [(&a, &b), (&b, &a)] {
            let old = first.items.values().next().unwrap();
            assert!(old.verified_opponent_lead_in().is_some());
            let captured = crate::plan_tactic_capture(first, second)
                .unwrap()
                .content
                .catalog;
            let imported = crate::plan_import(first, second).unwrap().catalog;
            for combined in [captured, imported] {
                let saved = &combined.items[&old.target];
                assert_eq!(saved.version, old.version);
                assert_eq!(saved.opponent_lead_in, LeadInEvidence::Conflicting);
                let again = crate::plan_tactic_capture(&combined, first)
                    .unwrap()
                    .content
                    .catalog;
                assert_eq!(
                    again.items[&old.target].opponent_lead_in,
                    LeadInEvidence::Conflicting
                );
            }
        }
    }
}
