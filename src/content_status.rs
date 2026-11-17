//! ContentStatus and its usage scenarios.

use serde::{Deserialize, Serialize};

/// Whether content can be selected for ordinary study.
///
/// # When to use
///
/// [`crate::plan_import`] quarantines unresolved, distinct tactics with the same
/// practice start. The application passes [`crate::ContentItem::active`] to queue
/// eligibility and retains the quarantined definitions for inspection.
///
/// Quarantine does not erase a target or make its definition structurally invalid.
/// It is an availability decision, separate from whether the learner got a move
/// right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    Active,
    Quarantined,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_catalog;
    use tempo_types::TargetId;

    #[test]
    fn quarantine_keeps_content_inspectable_but_excludes_it_from_new_selection() {
        let mut catalog = demo_catalog();
        let target = TargetId::from("t1");
        catalog.items.get_mut(&target).unwrap().status = ContentStatus::Quarantined;
        catalog.validate().unwrap();
        let selectable = catalog
            .items
            .values()
            .filter(|item| item.active())
            .map(|item| &item.target)
            .collect::<Vec<_>>();
        assert!(!selectable.contains(&&target));
        assert_eq!(selectable.len(), 2);
        assert_eq!(catalog.items[&target].definition.decisions.len(), 2);
    }
}
