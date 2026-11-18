//! ChessFeedback and its usage scenarios.

use crate::ChessAnswer;
use serde::{Deserialize, Serialize};

/// The explanation and automatic replies for one submitted move.
///
/// # When to use
///
/// The grader returns feedback and the UI shows it. Correct responses can include
/// automatic replies before the next prompt. An incorrect response has no autoplay
/// and leaves the learner on the same decision.
///
/// `correct` describes this response, not the whole attempt. After a previous
/// mistake a correction can be correct while Study retains `Again` as the attempt's
/// terminal assessment. See [`crate::ChessProgress`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessFeedback {
    pub correct: bool,
    pub message: String,
    pub autoplay: Vec<ChessAnswer>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::Grader;
    use tempo_types::TargetId;

    #[test]
    fn a_wrong_move_shows_feedback_without_playing_the_opponents_reply() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("t1")].definition;
        let start = ExactSequenceGrader.start(definition).unwrap();
        let miss = ExactSequenceGrader
            .answer(definition, &start.state, &"d2d4".into())
            .unwrap();
        let feedback: ChessFeedback = miss.feedback;
        assert!(!feedback.correct);
        assert!(feedback.autoplay.is_empty());
        assert_eq!(miss.state, start.state);
        let correction = ExactSequenceGrader
            .answer(definition, &miss.state, &"e2e4".into())
            .unwrap();
        assert!(correction.feedback.correct);
        assert_eq!(
            correction.feedback.autoplay,
            vec![ChessAnswer::from("e7e5")]
        );
    }
}
