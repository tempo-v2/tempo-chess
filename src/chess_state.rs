//! ChessState and its usage scenarios.

use serde::{Deserialize, Serialize};

/// The grader's resumable cursor into a definition's learner decisions.
///
/// # When to use
///
/// [`crate::ExactSequenceGrader`] produces this state after starting or answering.
/// Study stores it inside an attempt and supplies it on the next answer, so a
/// loaded attempt resumes at the same decision.
///
/// The zero-based cursor advances only after a correct move. It can equal the
/// number of decisions after completion. It contains no score: Study owns the
/// attempt's sticky terminal assessment separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessState {
    pub decision_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub played_mate: Option<crate::ChessAnswer>,
}

impl ChessState {
    pub fn at(decision_index: usize) -> Self {
        Self {
            decision_index,
            played_mate: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::{Grader, GradingError};
    use tempo_types::{Grade, TargetId};

    #[test]
    fn a_loaded_cursor_resumes_at_the_remaining_decision() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("t1")].definition;
        // A persisted attempt has already completed the first learner move.
        let state = ChessState::at(1);
        let miss = ExactSequenceGrader
            .answer(definition, &state, &"e2e4".into())
            .unwrap();
        assert_eq!(miss.state.decision_index, 1);
        assert_eq!(miss.assessment, Some(Grade::Again));
        let correction = ExactSequenceGrader
            .answer(definition, &state, &"g1f3".into())
            .unwrap();
        assert_eq!(correction.state.decision_index, 2);
        assert!(correction.complete);
        assert!(matches!(
            ExactSequenceGrader.answer(definition, &correction.state, &"g1f3".into()),
            Err(GradingError::InvalidState(_))
        ));
    }
}
