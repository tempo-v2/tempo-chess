//! Pure import planning; no learning or storage dependency.

use crate::tactic_record::is_source_token;
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
    let mut sources = BTreeMap::new();
    for item in current
        .items
        .values()
        .filter(|item| !item.definition.is_repertoire())
    {
        for token in item
            .provenance
            .iter()
            .filter(|token| is_source_token(token))
        {
            if sources
                .insert(token.as_str(), &item.target)
                .is_some_and(|target| target != &item.target)
            {
                return Err(ChessError::InvalidContent(format!(
                    "tactic source {token} has multiple owners"
                )));
            }
        }
    }
    for item in incoming.items.values() {
        // Check the existing source before comparing definitions. An exact
        // match under another ID must not mask a changed practice start.
        let existing = current.items.get(&item.target).or_else(|| {
            sources
                .get(item.target.0.as_str())
                .and_then(|owner| current.items.get(*owner))
        });
        if existing.is_some_and(|old| old.definition.kind != item.definition.kind) {
            return Err(ChessError::IdentityChanged(item.target.clone()));
        }
    }
    let mut catalog = current.clone();
    let mut changes = Vec::new();
    for original in incoming.items.values() {
        let mut item = original.clone();
        let previous_owner = sources.get(original.target.0.as_str()).copied();
        // Preserve named targets with their own learning history. For an alias,
        // prefer its prior owner while it still defines the same exercise.
        if !item.definition.is_repertoire() && !catalog.items.contains_key(&item.target) {
            let same_definition =
                |existing: &&crate::ContentItem| existing.definition == item.definition;
            let exact_match = previous_owner
                .and_then(|owner| catalog.items.get(owner))
                .filter(same_definition)
                .or_else(|| catalog.items.values().find(same_definition));
            if let Some(existing) = exact_match {
                item.target = existing.target.clone();
                item.label = existing.label.clone();
            }
        }
        if let Some(owner) = previous_owner.filter(|owner| **owner != item.target) {
            // A revised alias now has its own definition (or matches a third
            // target). Move its identity token; preserve retained provenance.
            let previous = catalog.items.get_mut(owner).expect("retained owner");
            previous
                .provenance
                .retain(|token| *token != original.target.0);
            changes.push(ContentChange {
                target: owner.clone(),
                kind: ChangeKind::Metadata,
            });
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
    for targets in starts.values() {
        let first = &catalog.items[&targets[0]].definition;
        let conflict = targets[1..]
            .iter()
            .any(|target| catalog.items[target].definition != *first);
        if !conflict {
            continue;
        }
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
    #[test]
    fn retained_aliases_guard_start_identity_and_split_on_conflicting_solutions() {
        use crate::{START_FEN, TacticRecord, apply_setup_move, compile_tactics};

        let source = |id: &str, fen: &str, uci: &str| TacticRecord {
            namespace: "synthetic".into(),
            source_id: id.into(),
            solver_fen: fen.into(),
            solution: vec![uci.into()],
            label: id.into(),
            provenance: vec![],
        };
        let a = source("a", START_FEN, "e2e4");
        let b = source("b", START_FEN, "e2e4");
        let other_fen = apply_setup_move(START_FEN, "d2d4").unwrap();
        let c = source("c", &other_fen, "d7d5");
        let first = compile_tactics(&[a.clone(), b.clone(), c]).unwrap();
        let merged = plan_import(&Catalog::default(), &first).unwrap().catalog;
        assert_eq!(merged.items.len(), 2);
        assert!(
            merged.items[&a.target_id()]
                .provenance
                .contains(&b.target_id().0)
        );
        let repeated = plan_import(&merged, &first).unwrap();
        assert_eq!(repeated.catalog, merged);
        assert!(
            repeated
                .changes
                .iter()
                .all(|change| change.kind == ChangeKind::Unchanged)
        );

        // The different starting position already exists with this response,
        // so comparing definitions before source identity would accept it.
        for source_id in ["a", "b"] {
            let moved = source(source_id, &other_fen, "d7d5");
            assert_eq!(
                plan_import(
                    &merged,
                    &compile_tactics(std::slice::from_ref(&moved)).unwrap()
                ),
                Err(ChessError::IdentityChanged(moved.target_id())),
            );
        }
        let revised = source("b", START_FEN, "d2d4");
        let split = plan_import(&merged, &compile_tactics(&[revised]).unwrap()).unwrap();
        assert_eq!(split.catalog.items.len(), 3);
        assert!(
            !split.catalog.items[&a.target_id()]
                .provenance
                .contains(&b.target_id().0)
        );
        for id in [a.target_id(), b.target_id()] {
            assert_eq!(split.catalog.items[&id].status, ContentStatus::Quarantined);
        }
    }
}
