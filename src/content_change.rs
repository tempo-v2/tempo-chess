//! ContentChange and its usage scenarios.

use crate::ChangeKind;
use serde::{Deserialize, Serialize};
use tempo_types::TargetId;

/// One target-level observation made while planning an import.
///
/// # When to use
///
/// The application reads these entries from [`crate::ContentPlan`] to decide which
/// learning records need creation or reset. The `target` identifies the retained
/// catalog entry, which may differ from an incoming duplicate tactic's source ID.
/// See [`ChangeKind`] for the meanings and the possibility of multiple entries for
/// the same target.
///
/// This is a proposed content effect, not a stored event or learner assessment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentChange {
    pub target: TargetId,
    pub kind: ChangeKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Catalog, demo_catalog, plan_import};

    #[test]
    fn duplicate_source_ids_report_changes_against_the_retained_target() {
        let current = demo_catalog();
        let mut duplicate = current.items[&TargetId::from("t1")].clone();
        duplicate.target = "source-specific-id".into();
        duplicate.provenance = vec!["Second collection".into()];
        let incoming = Catalog {
            items: [(duplicate.target.clone(), duplicate)].into(),
            lines: vec![],
        };
        let plan = plan_import(&current, &incoming).unwrap();
        assert!(plan.changes.contains(&ContentChange {
            target: "t1".into(),
            kind: ChangeKind::Metadata
        }));
        assert!(
            !plan
                .catalog
                .items
                .contains_key(&TargetId::from("source-specific-id"))
        );
    }
}
