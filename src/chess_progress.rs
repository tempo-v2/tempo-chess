//! ChessProgress and its usage scenarios.

use crate::{ChessFeedback, ChessPrompt, ChessState};

/// The result of one Study submission using the chess grader.
///
/// # When to use
///
/// The application receives this from [`tempo_study::submit()`], persists the updated
/// attempt and shows the feedback. Only `assessment: Some(...)` calls for a new
/// learning transition. An attempt may already have a terminal grade while this
/// particular result has no new assessment.
///
/// This distinction lets a learner correct a failed sequence without granting a
/// second learning credit for the same attempt.
pub type ChessProgress = tempo_study::Progress<ChessState, ChessPrompt, ChessFeedback>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChessAttempt, ExactSequenceGrader, demo_catalog};
    use tempo_study::{AttemptStatus, Grader};
    use tempo_types::{Grade, Revision, TargetId};

    #[test]
    fn correction_feedback_can_be_correct_without_emitting_a_second_assessment() {
        let catalog = demo_catalog();
        let item = &catalog.items[&TargetId::from("r1:p1")];
        let start = ExactSequenceGrader.start(&item.definition).unwrap();
        let attempt = ChessAttempt {
            id: "attempt-1".into(),
            dataset: "my-content".into(),
            target: item.target.clone(),
            content_version: item.version,
            revision: Revision(0),
            state: start.state,
            prompt: Some(start.prompt),
            terminal: None,
            status: AttemptStatus::Active,
        };
        let miss: ChessProgress = tempo_study::submit(
            &ExactSequenceGrader,
            &item.definition,
            &attempt,
            attempt.revision,
            &"d2d4".into(),
        )
        .unwrap();
        assert_eq!(miss.assessment, Some(Grade::Again));
        let correction: ChessProgress = tempo_study::submit(
            &ExactSequenceGrader,
            &item.definition,
            &miss.attempt,
            miss.attempt.revision,
            &"e2e4".into(),
        )
        .unwrap();
        assert!(correction.feedback.correct);
        assert_eq!(correction.assessment, None);
        assert_eq!(correction.attempt.terminal, Some(Grade::Again));
        assert_eq!(correction.attempt.status, AttemptStatus::Complete);
    }
}
