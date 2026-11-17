//! Pure import planning; no learning or storage dependency.

use crate::{
    Catalog, ChangeKind, ChessError, ContentChange, ContentKind, ContentPlan, ContentStatus,
    PositionKey,
};
use std::collections::BTreeMap;
use tempo_types::{ContentVersion, TargetId};

/// Merge incoming content into a reviewable, versioned proposal.
///
/// # When to use
///
/// Call this after parsing and before constructing application writes. Unmentioned
/// existing targets remain in the result. Metadata preserves versions; changed
/// definitions increment them. Identical tactics share provenance, and distinct
/// unresolved tactics with the same start are quarantined together.
///
/// Both inputs remain unchanged. The application owns learning resets and atomic
/// persistence; this function only reports content effects.
pub fn plan_import(current: &Catalog, incoming: &Catalog) -> Result<ContentPlan, ChessError> {
    current.validate()?;
    incoming.validate()?;
    let mut catalog = current.clone();
    let mut changes = Vec::new();
    for original in incoming.items.values() {
        let mut item = original.clone();
        // Identical tactic content imported under another source ID adds provenance to its target.
        if matches!(item.definition.kind, ContentKind::Tactic { .. }) {
            if let Some(existing) = catalog
                .items
                .values()
                .find(|v| v.definition == item.definition)
            {
                item.target = existing.target.clone();
            }
        }
        let kind = if let Some(old) = catalog.items.get(&item.target) {
            if old.definition.kind != item.definition.kind {
                return Err(ChessError::IdentityChanged(item.target));
            }
            for source in &old.provenance {
                if !item.provenance.contains(source) {
                    item.provenance.push(source.clone());
                }
            }
            item.provenance.sort();
            item.provenance.dedup();
            if old.definition != item.definition {
                item.version = ContentVersion(
                    old.version
                        .0
                        .checked_add(1)
                        .ok_or_else(|| ChessError::VersionExhausted(item.target.clone()))?,
                );
                ChangeKind::Semantic
            } else {
                item.version = old.version;
                if old.status == ContentStatus::Quarantined {
                    item.status = ContentStatus::Quarantined;
                }
                let mut normalized = old.clone();
                normalized.provenance.sort();
                normalized.provenance.dedup();
                if normalized == item {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::Metadata
                }
            }
        } else {
            item.version = ContentVersion(1);
            ChangeKind::Added
        };
        changes.push(ContentChange {
            target: item.target.clone(),
            kind,
        });
        catalog.items.insert(item.target.clone(), item);
    }
    for line in &incoming.lines {
        if let Some(old) = catalog.lines.iter_mut().find(|v| v.id == line.id) {
            *old = line.clone();
        } else {
            catalog.lines.push(line.clone());
        }
    }
    // Distinct unresolved tactics with the same practice start are quarantined together.
    let mut starts: BTreeMap<PositionKey, Vec<TargetId>> = BTreeMap::new();
    for item in catalog.items.values() {
        if let ContentKind::Tactic { practice_start } = &item.definition.kind {
            starts
                .entry(practice_start.clone())
                .or_default()
                .push(item.target.clone());
        }
    }
    for targets in starts.values().filter(|v| v.len() > 1) {
        for target in targets {
            let item = catalog
                .items
                .get_mut(target)
                .expect("target collected from catalog");
            if item.status != ContentStatus::Quarantined {
                item.status = ContentStatus::Quarantined;
                changes.push(ContentChange {
                    target: target.clone(),
                    kind: ChangeKind::Quarantined,
                });
            }
        }
    }
    catalog.validate()?;
    Ok(ContentPlan { catalog, changes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_catalog;

    #[test]
    fn metadata_preserves_version_while_response_change_versions_only_target() {
        let current = demo_catalog();
        let mut incoming = current.clone();
        incoming
            .items
            .get_mut(&TargetId::from("r1:p1"))
            .unwrap()
            .label = "Annotation updated".into();
        let metadata = plan_import(&current, &incoming).unwrap();
        assert_eq!(
            metadata.catalog.items[&TargetId::from("r1:p1")].version,
            ContentVersion(1)
        );
        assert!(metadata.changes.contains(&ContentChange {
            target: "r1:p1".into(),
            kind: ChangeKind::Metadata
        }));
        incoming
            .items
            .get_mut(&TargetId::from("r1:p1"))
            .unwrap()
            .definition
            .decisions[0]
            .expected = "d2d4".into();
        let semantic = plan_import(&current, &incoming).unwrap();
        assert_eq!(
            semantic.catalog.items[&TargetId::from("r1:p1")].version,
            ContentVersion(2)
        );
        assert_eq!(
            semantic.catalog.items[&TargetId::from("r1:p2")].version,
            ContentVersion(1)
        );
        assert!(semantic.changes.contains(&ContentChange {
            target: "r1:p1".into(),
            kind: ChangeKind::Semantic
        }));
    }

    #[test]
    fn duplicate_tactic_adds_provenance_and_distinct_same_start_is_quarantined() {
        let current = demo_catalog();
        let mut duplicate = current.items[&TargetId::from("t1")].clone();
        duplicate.target = "imported-id".into();
        duplicate.provenance = vec!["another source".into()];
        let mut incoming = Catalog {
            items: [(duplicate.target.clone(), duplicate)].into(),
            lines: vec![],
        };
        let merged = plan_import(&current, &incoming).unwrap();
        assert_eq!(merged.catalog.items.len(), current.items.len());
        assert!(
            merged.catalog.items[&TargetId::from("t1")]
                .provenance
                .contains(&"another source".into())
        );
        let changed = incoming
            .items
            .get_mut(&TargetId::from("imported-id"))
            .unwrap();
        changed.definition.decisions[1].expected = "f1c4".into();
        let conflicted = plan_import(&current, &incoming).unwrap();
        assert_eq!(
            conflicted.catalog.items[&TargetId::from("t1")].status,
            ContentStatus::Quarantined
        );
        assert_eq!(
            conflicted.catalog.items[&TargetId::from("imported-id")].status,
            ContentStatus::Quarantined
        );
        assert_eq!(
            conflicted.catalog.items[&TargetId::from("r1:p1")].status,
            ContentStatus::Active
        );
    }
}
