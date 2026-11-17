//! ChessAnswer and its usage scenarios.

use serde::{Deserialize, Serialize};

/// One learner move in UCI coordinate notation, such as `e2e4` or `e7e8q`.
///
/// # When to use
///
/// The application converts board input into this value and passes it to Study.
/// [`crate::ExactSequenceGrader`] compares it with a decision's prescribed move.
/// It also represents automatic replies in [`crate::Decision`] and feedback.
///
/// `From<&str>` only packages text. The grader rejects malformed move notation as
/// an input error, rather than counting it as an incorrect chess answer. A
/// well-shaped move can still be illegal; this fixture grader checks prescribed
/// moves, not board legality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessAnswer {
    pub uci: String,
}
impl From<&str> for ChessAnswer {
    fn from(value: &str) -> Self {
        Self { uci: value.into() }
    }
}

pub(crate) fn valid_uci(value: &str) -> bool {
    let v = value.as_bytes();
    (v.len() == 4 || (v.len() == 5 && b"qrbn".contains(&v[4])))
        && (b'a'..=b'h').contains(&v[0])
        && (b'1'..=b'8').contains(&v[1])
        && (b'a'..=b'h').contains(&v[2])
        && (b'1'..=b'8').contains(&v[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::{Grader, GradingError};
    use tempo_types::{Grade, TargetId};

    #[test]
    fn transport_text_becomes_an_answer_but_validation_happens_at_the_grader() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("r1:p1")].definition;
        let start = ExactSequenceGrader.start(definition).unwrap();
        let typed_move = ChessAnswer::from("e2e4");
        let accepted = ExactSequenceGrader
            .answer(definition, &start.state, &typed_move)
            .unwrap();
        assert_eq!(accepted.assessment, Some(Grade::Good));
        let malformed = ChessAnswer::from("pawn to e4");
        assert!(matches!(
            ExactSequenceGrader.answer(definition, &start.state, &malformed),
            Err(GradingError::InvalidAnswer(_))
        ));
    }
}
