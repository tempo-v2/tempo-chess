//! ExactSequenceGrader and its usage scenarios.

use tempo_study::{GradeStep, Grader, GraderStart, GradingError};

use crate::chess_answer::valid_uci;
use crate::{ChessAnswer, ChessDefinition, ChessFeedback, ChessPrompt, ChessState};
use tempo_types::Grade;

/// Matches prescribed UCI moves. It does not prove legality or objective chess quality.
/// Grades prescribed UCI moves through Study's generic [`Grader`] interface.
///
/// # When to use
///
/// Supply this grader when issuing or answering a [`ChessDefinition`] using the
/// `ExactSequenceV1` scheme. `start` returns the first state/prompt; `answer` returns
/// one response's feedback, new cursor, optional assessment and completion flag.
/// It does not prove legality or objective chess quality.
///
/// A correct intermediate move has no assessment. A wrong move proposes `Again`
/// and leaves the cursor in place; the final correct move proposes `Good`. Study
/// owns the sticky, at-most-once whole-attempt assessment, so always use
/// [`tempo_study::submit()`] for attempt progression and correction flows.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExactSequenceGrader;
fn prompt(definition: &ChessDefinition, index: usize) -> ChessPrompt {
    ChessPrompt {
        fen: definition.decisions[index].fen.clone(),
        instruction: "Play the prescribed move".into(),
        decision_index: index,
        decision_count: definition.decisions.len(),
    }
}
impl Grader for ExactSequenceGrader {
    type Definition = ChessDefinition;
    type State = ChessState;
    type Answer = ChessAnswer;
    type Prompt = ChessPrompt;
    type Feedback = ChessFeedback;
    fn start(
        &self,
        definition: &ChessDefinition,
    ) -> Result<GraderStart<ChessState, ChessPrompt>, GradingError> {
        definition
            .validate()
            .map_err(|e| GradingError::InvalidDefinition(e.to_string()))?;
        Ok(GraderStart {
            state: ChessState::at(0),
            prompt: prompt(definition, 0),
        })
    }
    fn answer(
        &self,
        definition: &ChessDefinition,
        state: &ChessState,
        answer: &ChessAnswer,
    ) -> Result<GradeStep<ChessState, ChessPrompt, ChessFeedback>, GradingError> {
        definition
            .validate()
            .map_err(|e| GradingError::InvalidDefinition(e.to_string()))?;
        if !valid_uci(&answer.uci) {
            return Err(GradingError::InvalidAnswer(
                "expected a UCI-shaped move".into(),
            ));
        }
        let decision = definition
            .decisions
            .get(state.decision_index)
            .ok_or_else(|| GradingError::InvalidState("decision is outside the exercise".into()))?;
        let correct = answer == &decision.expected;
        let next_index = state.decision_index + usize::from(correct);
        let complete = next_index == definition.decisions.len();
        Ok(GradeStep {
            state: ChessState::at(next_index),
            prompt: if complete {
                None
            } else {
                Some(prompt(definition, next_index))
            },
            feedback: ChessFeedback {
                correct,
                message: if correct {
                    "Correct".into()
                } else {
                    "Incorrect; try the prescribed move".into()
                },
                autoplay: if correct {
                    decision.autoplay.clone()
                } else {
                    Vec::new()
                },
            },
            assessment: if !correct {
                Some(Grade::Again)
            } else if complete {
                Some(Grade::Good)
            } else {
                None
            },
            complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::convert::Infallible;
    use tempo_study::{
        AttemptStatus, Availability, Exercise, GradingError, QuestionSource, StudyError,
        next_available_exercise_if_any,
    };
    use tempo_types::{Grade, Revision, TargetId};

    struct Source(Option<Exercise<ChessDefinition>>);
    impl QuestionSource for Source {
        type Definition = ChessDefinition;
        type Error = Infallible;
        fn next(&mut self) -> Result<Availability<Exercise<ChessDefinition>>, Infallible> {
            Ok(self
                .0
                .take()
                .map(Availability::Ready)
                .unwrap_or(Availability::Empty))
        }
    }
    fn tactic() -> (ChessDefinition, ChessAttempt) {
        let catalog = demo_catalog();
        let exercise = catalog.items[&TargetId::from("t1")].exercise();
        let definition = exercise.definition.clone();
        let Availability::Ready(started) = next_available_exercise_if_any(
            &mut Source(Some(exercise)),
            &ExactSequenceGrader,
            "dataset".into(),
            "a1".into(),
        )
        .unwrap() else {
            panic!("fixture ready")
        };
        (definition, started.attempt)
    }

    #[test]
    fn clean_tactic_assesses_only_after_both_learner_decisions() {
        let (definition, attempt) = tactic();
        let first = tempo_study::submit(
            &ExactSequenceGrader,
            &definition,
            &attempt,
            Revision(0),
            &"e2e4".into(),
        )
        .unwrap();
        assert_eq!(first.assessment, None);
        assert_eq!(first.feedback.autoplay, vec![ChessAnswer::from("e7e5")]);
        assert_eq!(first.attempt.prompt.as_ref().unwrap().fen, SECOND_FEN);
        let last = tempo_study::submit(
            &ExactSequenceGrader,
            &definition,
            &first.attempt,
            Revision(1),
            &"g1f3".into(),
        )
        .unwrap();
        assert_eq!(last.assessment, Some(Grade::Good));
        assert_eq!(last.attempt.status, AttemptStatus::Complete);
    }

    #[test]
    fn miss_on_either_decision_is_sticky_and_correction_is_not_credit() {
        for miss_index in [0, 1] {
            let (definition, mut attempt) = tactic();
            let mut emitted = Vec::new();
            for (index, correct) in ["e2e4", "g1f3"].into_iter().enumerate() {
                if index == miss_index {
                    let miss = tempo_study::submit(
                        &ExactSequenceGrader,
                        &definition,
                        &attempt,
                        attempt.revision,
                        &"a2a3".into(),
                    )
                    .unwrap();
                    emitted.extend(miss.assessment);
                    attempt = miss.attempt;
                }
                let next = tempo_study::submit(
                    &ExactSequenceGrader,
                    &definition,
                    &attempt,
                    attempt.revision,
                    &correct.into(),
                )
                .unwrap();
                emitted.extend(next.assessment);
                attempt = next.attempt;
            }
            assert_eq!(emitted, vec![Grade::Again]);
            assert_eq!(attempt.terminal, Some(Grade::Again));
            assert_eq!(attempt.status, AttemptStatus::Complete);
        }
    }

    #[test]
    fn invalid_answer_is_a_protocol_error_and_does_not_grade() {
        let (definition, attempt) = tactic();
        assert!(matches!(
            tempo_study::submit(
                &ExactSequenceGrader,
                &definition,
                &attempt,
                Revision(0),
                &"queen takes".into()
            ),
            Err(StudyError::Grading(GradingError::InvalidAnswer(_)))
        ));
        assert_eq!(attempt.revision, Revision(0));
        assert_eq!(attempt.terminal, None);
    }
}
