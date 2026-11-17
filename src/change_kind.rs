//! ChangeKind and its usage scenarios.

use serde::{Deserialize, Serialize};

/// How one import operation affects a target.
///
/// # When to use
///
/// [`crate::plan_import`] produces change entries; the application uses them to
/// prepare content and learning writes together. `Added` initializes learning,
/// `Semantic` signals a new content version and a learning reset, and `Metadata`
/// or `Unchanged` preserves progress.
///
/// `Quarantined` signals a selection restriction. A target may have an earlier
/// `Added` or `Semantic` entry and a later `Quarantined` entry in the same plan;
/// these entries are not a unique map from target to one change kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Metadata,
    Semantic,
    Unchanged,
    Quarantined,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo_catalog, plan_import};
    use tempo_types::TargetId;

    #[test]
    fn application_selects_learning_resets_from_semantic_changes_only() {
        let current = demo_catalog();
        let mut incoming = current.clone();
        incoming
            .items
            .get_mut(&TargetId::from("r1:p1"))
            .unwrap()
            .label = "Clearer name".into();
        incoming
            .items
            .get_mut(&TargetId::from("r1:p2"))
            .unwrap()
            .definition
            .decisions[0]
            .expected = "f1c4".into();
        let plan = plan_import(&current, &incoming).unwrap();
        let reset_targets = plan
            .changes
            .iter()
            .filter(|change| matches!(change.kind, ChangeKind::Semantic))
            .map(|change| change.target.clone())
            .collect::<Vec<_>>();
        assert_eq!(reset_targets, vec![TargetId::from("r1:p2")]);
    }
}
