use tempo_study::{GradeStep, Grader, GraderStart, GradingError};

use crate::model::valid_uci;
use crate::{ChessAnswer, ChessDefinition, ChessFeedback, ChessPrompt, ChessState};

/// Matches prescribed UCI moves. It does not prove legality or objective chess quality.
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
            state: ChessState { decision_index: 0 },
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
            state: ChessState {
                decision_index: next_index,
            },
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
