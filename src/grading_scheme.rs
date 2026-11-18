//! GradingScheme and its usage scenarios.

use serde::{Deserialize, Serialize};

/// The persisted identity of the rules used to grade a definition.
///
/// # When to use
///
/// Content import chooses a scheme; a grader adapter can match it to an
/// implementation when loading persisted content. Persist this value, not a
/// function pointer or callback. The current single scheme is implemented by
/// [`crate::ExactSequenceGrader`].
///
/// Serde rejects unknown variant names. A future change of scheme is a semantic
/// change to [`crate::ChessDefinition`] and therefore requires a new content
/// version through [`crate::plan_import`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradingScheme {
    ExactSequenceV1,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::Grader;
    use tempo_types::TargetId;

    #[test]
    fn a_loaded_scheme_selects_the_grader_without_persisting_a_callback() {
        let catalog = demo_catalog();
        let definition = &catalog.items[&TargetId::from("t1")].definition;
        let start = match definition.scheme {
            GradingScheme::ExactSequenceV1 => ExactSequenceGrader.start(definition),
        }
        .unwrap();
        assert_eq!(start.prompt.decision_count, definition.decisions.len());
        assert_eq!(start.prompt.fen, definition.decisions[0].fen);
    }
}
