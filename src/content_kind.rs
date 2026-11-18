//! ContentKind and its usage scenarios.

use crate::PositionKey;
use serde::{Deserialize, Serialize};

/// The chess identity and exercise family of a learning target.
///
/// # When to use
///
/// Content import assigns this value when deciding what is being learned.
/// [`crate::Catalog`] checks repertoire position uniqueness; [`crate::plan_import`]
/// uses it to reject changes that would turn an existing target into a different
/// identity.
///
/// A repertoire target represents one position in one named repertoire and has
/// one learner decision. A tactic has one or more decisions but still represents
/// one target and one whole-exercise assessment. Sharing a starting board does not
/// make a tactic and a repertoire target the same learning item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentKind {
    Repertoire {
        repertoire: String,
        position: PositionKey,
    },
    Tactic {
        practice_start: PositionKey,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExactSequenceGrader, demo_catalog};
    use tempo_study::Grader;
    use tempo_types::TargetId;

    #[test]
    fn repertoire_and_tactic_can_share_a_start_but_ask_different_work() {
        let catalog = demo_catalog();
        let repertoire = &catalog.items[&TargetId::from("r1:p1")];
        let tactic = &catalog.items[&TargetId::from("t1")];
        let ContentKind::Repertoire { position, .. } = &repertoire.definition.kind else {
            panic!("repertoire fixture");
        };
        let ContentKind::Tactic { practice_start } = &tactic.definition.kind else {
            panic!("tactic fixture");
        };
        assert_eq!(position, practice_start);
        assert_ne!(repertoire.target, tactic.target);
        assert_eq!(
            ExactSequenceGrader
                .start(&repertoire.definition)
                .unwrap()
                .prompt
                .decision_count,
            1
        );
        assert_eq!(
            ExactSequenceGrader
                .start(&tactic.definition)
                .unwrap()
                .prompt
                .decision_count,
            2
        );
    }
}
