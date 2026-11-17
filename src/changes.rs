use crate::{Catalog, ChessError, ContentKind, ContentStatus, PositionKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tempo_types::{ContentVersion, TargetId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Metadata,
    Semantic,
    Unchanged,
    Quarantined,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentChange {
    pub target: TargetId,
    pub kind: ChangeKind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPlan {
    pub catalog: Catalog,
    pub changes: Vec<ContentChange>,
}

/// Proposes a content replacement/import. Learning is deliberately absent from this API.
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
