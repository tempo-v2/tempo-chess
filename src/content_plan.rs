//! ContentPlan and its usage scenarios.

use crate::{Catalog, ContentChange};
use serde::{Deserialize, Serialize};

/// An import proposal containing the complete merged catalog and its effects.
///
/// # When to use
///
/// [`crate::plan_import`] returns this value without modifying either input.
/// The application uses `changes` to compute corresponding learning updates, then
/// commits those updates and `catalog` atomically. Chess never receives learning
/// state or a persistence handle.
///
/// The catalog contains retained existing content as well as incoming content;
/// it is not just the imported subset. A plan is reviewable before it is applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPlan {
    pub catalog: Catalog,
    pub changes: Vec<ContentChange>,
}

#[cfg(test)]
mod tests {
    use crate::{Catalog, demo_catalog, plan_import};
    use tempo_types::TargetId;

    #[test]
    fn a_plan_is_a_complete_reviewable_merge_and_does_not_mutate_its_inputs() {
        let current = demo_catalog();
        let original = current.clone();
        let mut edited = current.items[&TargetId::from("r1:p1")].clone();
        edited.label = "A reviewed label".into();
        let incoming = Catalog {
            items: [(edited.target.clone(), edited)].into(),
            lines: vec![],
        };
        let plan = plan_import(&current, &incoming).unwrap();
        assert_eq!(current, original);
        assert_eq!(incoming.items.len(), 1);
        assert_eq!(plan.catalog.items.len(), 3);
        assert_eq!(plan.catalog.lines, original.lines);
        assert_eq!(
            plan.catalog.items[&TargetId::from("r1:p1")].label,
            "A reviewed label"
        );
        assert_eq!(
            plan.catalog.items[&TargetId::from("t1")],
            original.items[&TargetId::from("t1")]
        );
    }
}
