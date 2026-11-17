//! ChessDefinition and its usage scenarios.

use crate::chess_answer::valid_uci;
use crate::{ChessError, ContentKind, Decision, GradingScheme};
use serde::{Deserialize, Serialize};

/// The complete chess-specific instructions for presenting and grading one target.
///
/// # When to use
///
/// Pass a definition to a [`tempo_study::Grader`]. Content import produces it and
/// [`crate::ContentItem`] stores it alongside identity, version and metadata.
/// The grader needs the definition, not the surrounding catalog or learning state.
///
/// Repertoire definitions contain exactly one [`Decision`]; tactics contain at
/// least one. `validate()` checks these counts, six-field FEN shape and UCI-shaped
/// moves. It does not prove board legality or that a prescribed move is best.
///
/// ```
/// use tempo_chess::{demo_catalog, ExactSequenceGrader};
/// use tempo_study::Grader;
/// use tempo_types::TargetId;
///
/// let catalog = demo_catalog();
/// let definition = &catalog.items[&TargetId::from("t1")].definition;
/// definition.validate()?;
/// let first = ExactSequenceGrader.start(definition)?;
/// assert_eq!(first.prompt.decision_count, 2);
/// assert_eq!(first.state.decision_index, 0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessDefinition {
    pub scheme: GradingScheme,
    pub kind: ContentKind,
    pub decisions: Vec<Decision>,
}
impl ChessDefinition {
    pub fn is_repertoire(&self) -> bool {
        matches!(self.kind, ContentKind::Repertoire { .. })
    }
    pub fn validate(&self) -> Result<(), ChessError> {
        if self.decisions.is_empty() || (self.is_repertoire() && self.decisions.len() != 1) {
            return Err(ChessError::InvalidContent(
                "one repertoire decision or a nonempty tactic is required".into(),
            ));
        }
        for decision in &self.decisions {
            if decision.fen.split_whitespace().count() != 6
                || !valid_uci(&decision.expected.uci)
                || decision.autoplay.iter().any(|m| !valid_uci(&m.uci))
            {
                return Err(ChessError::InvalidContent(
                    "expected six FEN fields and UCI-shaped moves; legality is not checked".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_catalog;
    use tempo_types::TargetId;

    #[test]
    fn repertoire_is_one_question_while_a_tactic_can_hold_the_whole_sequence() {
        let catalog = demo_catalog();
        let tactic = &catalog.items[&TargetId::from("t1")].definition;
        assert_eq!(tactic.decisions.len(), 2);
        tactic.validate().unwrap();
        let mut repertoire = catalog.items[&TargetId::from("r1:p1")].definition.clone();
        repertoire.decisions = tactic.decisions.clone();
        assert!(matches!(
            repertoire.validate(),
            Err(ChessError::InvalidContent(_))
        ));
        let mut empty_tactic = tactic.clone();
        empty_tactic.decisions.clear();
        assert!(matches!(
            empty_tactic.validate(),
            Err(ChessError::InvalidContent(_))
        ));
    }
}
