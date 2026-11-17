//! ChessAttempt and its usage scenarios.

use crate::{ChessPrompt, ChessState};

/// Study's generic attempt specialized to chess state and prompts.
///
/// # When to use
///
/// Use this alias at a chess application boundary instead of repeating the generic
/// parameters. Study creates and advances it; persistence can wrap it with issue
/// time and learning context without adding those concerns to Study.
///
/// An attempt combines target/content version, response revision, the grader's
/// cursor, presentation status and an optional terminal assessment. Completing a
/// previously failed attempt does not change its terminal grade.
pub type ChessAttempt = tempo_study::Attempt<ChessState, ChessPrompt>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::{AttemptStatus, Grader};
    use tempo_types::{Grade, Revision, TargetId};

    #[test]
    fn a_chess_attempt_uses_generic_study_progression_and_keeps_its_content_stamp() {
        let catalog = demo_catalog();
        let exercise = catalog.items[&TargetId::from("r1:p1")].exercise();
        let start = ExactSequenceGrader.start(&exercise.definition).unwrap();
        let attempt = ChessAttempt {
            id: "attempt-1".into(),
            dataset: "my-content".into(),
            target: exercise.target.clone(),
            content_version: exercise.version,
            revision: Revision(0),
            state: start.state,
            prompt: Some(start.prompt),
            terminal: None,
            status: AttemptStatus::Active,
        };
        let progress = tempo_study::submit(
            &ExactSequenceGrader,
            &exercise.definition,
            &attempt,
            attempt.revision,
            &"e2e4".into(),
        )
        .unwrap();
        assert_eq!(progress.attempt.target, exercise.target);
        assert_eq!(progress.attempt.content_version, exercise.version);
        assert_eq!(progress.attempt.revision, Revision(1));
        assert_eq!(progress.attempt.status, AttemptStatus::Complete);
        assert_eq!(progress.assessment, Some(Grade::Good));
    }
}
