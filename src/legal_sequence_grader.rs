//! Real-board validation around the existing prescribed-move Study grader.

use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;
use crate::{
    ChessAnswer, ChessDefinition, ChessFeedback, ChessPrompt, ChessState, ExactSequenceGrader,
};
use tempo_study::{GradeStep, Grader, GraderStart, GradingError};

/// Grade a prescribed sequence only after the submitted move is legal.
#[derive(Clone, Copy, Debug, Default)]
pub struct LegalSequenceGrader;

impl Grader for LegalSequenceGrader {
    type Definition = ChessDefinition;
    type State = ChessState;
    type Answer = ChessAnswer;
    type Prompt = ChessPrompt;
    type Feedback = ChessFeedback;

    fn start(
        &self,
        definition: &ChessDefinition,
    ) -> Result<GraderStart<ChessState, ChessPrompt>, GradingError> {
        let start = ExactSequenceGrader.start(definition)?;
        parse_position(&start.prompt.fen, true)
            .map_err(|error| GradingError::InvalidDefinition(error.to_string()))?;
        Ok(start)
    }

    fn answer(
        &self,
        definition: &ChessDefinition,
        state: &ChessState,
        answer: &ChessAnswer,
    ) -> Result<GradeStep<ChessState, ChessPrompt, ChessFeedback>, GradingError> {
        let decision = definition
            .decisions
            .get(state.decision_index)
            .ok_or_else(|| GradingError::InvalidState("decision is outside the exercise".into()))?;
        let board = parse_position(&decision.fen, true)
            .map_err(|error| GradingError::InvalidDefinition(error.to_string()))?;
        legal_move(&board, &answer.uci)
            .map_err(|error| GradingError::InvalidAnswer(error.to_string()))?;
        ExactSequenceGrader.answer(definition, state, answer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidedMoveError {
    InvalidDefinition,
    InvalidState,
    TryShownMove,
}

/// Validate reproduction of the one shown repertoire decision in Teach or Repair.
pub fn acknowledge_shown_move(
    definition: &ChessDefinition,
    state: &ChessState,
    answer: &ChessAnswer,
) -> Result<ChessState, GuidedMoveError> {
    let [decision] = definition.decisions.as_slice() else {
        return Err(GuidedMoveError::InvalidDefinition);
    };
    if !definition.is_repertoire() {
        return Err(GuidedMoveError::InvalidDefinition);
    }
    if state.decision_index != 0 {
        return Err(GuidedMoveError::InvalidState);
    }
    let board =
        parse_position(&decision.fen, true).map_err(|_| GuidedMoveError::InvalidDefinition)?;
    legal_move(&board, &decision.expected.uci).map_err(|_| GuidedMoveError::InvalidDefinition)?;
    if answer != &decision.expected {
        return Err(GuidedMoveError::TryShownMove);
    }
    Ok(ChessState { decision_index: 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo_catalog, project_position};
    use tempo_types::{Grade, TargetId};

    #[test]
    fn guided_acknowledgement_accepts_only_the_legal_shown_repertoire_move() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("r1:p1")].definition;
        let start = LegalSequenceGrader.start(definition).unwrap();
        let shown = &definition.decisions[0].expected;
        for incorrect in ["not-a-move", "e2e5"] {
            assert_eq!(
                acknowledge_shown_move(definition, &start.state, &incorrect.into()),
                Err(GuidedMoveError::TryShownMove)
            );
        }
        assert_eq!(
            acknowledge_shown_move(definition, &start.state, shown),
            Ok(ChessState { decision_index: 1 })
        );
        let mut invalid = definition.clone();
        invalid.decisions[0].expected = "e2e5".into();
        assert_eq!(
            acknowledge_shown_move(&invalid, &start.state, &"e2e5".into()),
            Err(GuidedMoveError::InvalidDefinition)
        );
    }

    #[test]
    fn malformed_and_illegal_moves_do_not_enter_the_miss_path() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("t1")].definition;
        let start = LegalSequenceGrader.start(definition).unwrap();
        for bad in ["illegal input", "e2e5", "e7e5"] {
            assert!(matches!(
                LegalSequenceGrader.answer(definition, &start.state, &bad.into()),
                Err(GradingError::InvalidAnswer(_))
            ));
        }
        let miss = LegalSequenceGrader
            .answer(definition, &start.state, &"d2d4".into())
            .unwrap();
        assert_eq!(miss.assessment, Some(Grade::Again));
        assert_eq!(miss.state, start.state);
        assert_eq!(
            project_position(definition, &miss.state, true)
                .unwrap()
                .display_fen,
            start.prompt.fen
        );
        let correct = LegalSequenceGrader
            .answer(definition, &miss.state, &"e2e4".into())
            .unwrap();
        assert!(correct.assessment.is_none());
        assert_eq!(correct.feedback.autoplay, vec!["e7e5".into()]);
    }
}
