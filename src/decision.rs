//! Decision and its usage scenarios.

use crate::ChessAnswer;
use serde::{Deserialize, Serialize};

/// One learner decision inside a chess exercise, followed by optional replies.
///
/// # When to use
///
/// A parser constructs decisions while turning a line or puzzle into a
/// [`crate::ChessDefinition`]. The grader presents `fen`, compares the learner's
/// move with `expected`, and returns `autoplay` only after the correct response.
/// The next decision supplies its own board position.
///
/// `autoplay` contains context, usually the opponent's reply. Those moves are not
/// additional questions or additional assessments. A tactic with seven decisions
/// still has one overall grade.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub fen: String,
    pub expected: ChessAnswer,
    pub autoplay: Vec<ChessAnswer>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChessDefinition, ContentKind, ExactSequenceGrader, GradingScheme, PositionKey, START_FEN,
    };
    use tempo_study::Grader;
    use tempo_types::Grade;

    #[test]
    fn automatic_opponent_replies_do_not_add_learner_decisions() {
        let definition = ChessDefinition {
            scheme: GradingScheme::ExactSequenceV1,
            kind: ContentKind::Tactic {
                practice_start: PositionKey("start".into()),
            },
            decisions: vec![Decision {
                fen: START_FEN.into(),
                expected: "e2e4".into(),
                autoplay: vec!["e7e5".into()],
            }],
        };
        let start = ExactSequenceGrader.start(&definition).unwrap();
        let result = ExactSequenceGrader
            .answer(&definition, &start.state, &"e2e4".into())
            .unwrap();
        assert_eq!(start.prompt.decision_count, 1);
        assert_eq!(result.feedback.autoplay, vec![ChessAnswer::from("e7e5")]);
        assert_eq!(result.assessment, Some(Grade::Good));
        assert!(result.complete);
        assert!(result.prompt.is_none());
    }
}
