//! ChessPrompt and its usage scenarios.

use serde::{Deserialize, Serialize};

/// The board and instructions to show for the next learner decision.
///
/// # When to use
///
/// The grader produces prompts, Study carries the current one, and presentation
/// code displays it. `decision_index` is zero-based; a UI may display index plus
/// one out of `decision_count`. The expected answer stays in the definition.
///
/// A prompt describes presentation. The authoritative progress cursor is
/// [`crate::ChessState`]; a UI should not manufacture state from the displayed board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessPrompt {
    pub fen: String,
    pub instruction: String,
    pub decision_index: usize,
    pub decision_count: usize,
}

#[cfg(test)]
mod tests {
    use crate::{ExactSequenceGrader, SECOND_FEN, START_FEN, demo_catalog};
    use tempo_study::Grader;
    use tempo_types::TargetId;

    #[test]
    fn presentation_follows_the_next_decision_and_disappears_at_completion() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("t1")].definition;
        let start = ExactSequenceGrader.start(definition).unwrap();
        assert_eq!(start.prompt.fen, START_FEN);
        assert_eq!(start.prompt.decision_index + 1, 1);
        let first = ExactSequenceGrader
            .answer(definition, &start.state, &"e2e4".into())
            .unwrap();
        let prompt = first.prompt.as_ref().unwrap();
        assert_eq!(prompt.fen, SECOND_FEN);
        assert_eq!(prompt.decision_index + 1, prompt.decision_count);
        let last = ExactSequenceGrader
            .answer(definition, &first.state, &"g1f3".into())
            .unwrap();
        assert!(last.prompt.is_none());
    }
}
