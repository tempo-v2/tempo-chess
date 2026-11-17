//! RepertoireLine and its usage scenarios.

use serde::{Deserialize, Serialize};
use tempo_types::TargetId;

/// An ordered sequence of target occurrences providing repertoire context.
///
/// # When to use
///
/// PGN import produces lines; application queue adapters use their order to form
/// study blocks. Each target still has its own definition and learning schedule.
/// The line is not a single multi-decision tactic.
///
/// Repeated positions and transpositions share target identities. `targets` may
/// therefore repeat a target, preserving the route through a finite line even if
/// the underlying position graph has cycles. Catalog validation checks that every
/// referenced target belongs to this line's repertoire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireLine {
    pub id: String,
    pub repertoire: String,
    /// Ordered occurrences, deliberately allowing a target to appear repeatedly.
    pub targets: Vec<TargetId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_catalog;
    use std::collections::BTreeSet;

    #[test]
    fn repeated_line_occurrences_reuse_the_same_position_target() {
        let mut catalog = demo_catalog();
        let line = RepertoireLine {
            id: "return-to-position".into(),
            repertoire: "r1".into(),
            targets: vec!["r1:p1".into(), "r1:p2".into(), "r1:p1".into()],
        };
        catalog.lines.push(line.clone());
        catalog.validate().unwrap();
        let distinct_targets = line.targets.iter().collect::<BTreeSet<_>>();
        assert_eq!(line.targets.len(), 3);
        assert_eq!(distinct_targets.len(), 2);
        assert_eq!(
            catalog.items[&line.targets[0]],
            catalog.items[&line.targets[2]]
        );
    }
}
