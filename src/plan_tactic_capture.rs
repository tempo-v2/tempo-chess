//! Merge explicitly captured legal tactics without changing existing definitions.

use crate::parsed_pgn::invalid;
use crate::tactic_record::is_source_token;
use crate::{
    Catalog, ChangeKind, ChessError, ContentChange, ContentItem, ContentKind, ContentPlan,
    PositionKey,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use tempo_types::{ContentVersion, TargetId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TacticCapturePlan {
    pub content: ContentPlan,
    /// Number of captured positions that have more than one distinct solution.
    pub alternate_solutions: usize,
}

/// Match tactics by canonical solver position and complete UCI line. An exact
/// match gains provenance while keeping its target, definition and version.
/// An authoritative source token cannot be reused for different content.
pub fn plan_tactic_capture(
    current: &Catalog,
    incoming: &Catalog,
) -> Result<TacticCapturePlan, ChessError> {
    incoming.validate()?;
    if !incoming.lines.is_empty() {
        return Err(invalid("tactic capture cannot change repertoire routes"));
    }
    plan_tactic_items(current, incoming.items.values())
}

pub(super) fn plan_tactic_items<'a>(
    current: &Catalog,
    incoming: impl IntoIterator<Item = &'a ContentItem>,
) -> Result<TacticCapturePlan, ChessError> {
    let mut index = CaptureIndex::default();
    for item in current.items.values() {
        if let Some(identity) = TacticIdentity::from_item(item) {
            index.insert(item, identity)?;
        }
    }

    let mut catalog = current.clone();
    let mut changes = Vec::new();
    let mut captured_positions = BTreeSet::new();
    for item in incoming {
        let identity = TacticIdentity::from_item(item)
            .filter(|_| item.active())
            .ok_or_else(|| invalid("tactic capture requires active tactics"))?;
        captured_positions.insert(identity.position.clone());
        let existing = index.find(item, &identity)?;
        let change = if let Some(target) = existing {
            let retained = catalog.items.get_mut(&target).expect("indexed tactic");
            let before = retained.provenance.clone();
            let adds_context =
                retained.opponent_lead_in.is_none() && item.opponent_lead_in.is_some();
            if adds_context {
                retained.opponent_lead_in.clone_from(&item.opponent_lead_in);
            }
            retained.provenance.extend(item.provenance.iter().cloned());
            if is_source_token(&item.target.0) {
                retained.provenance.push(item.target.0.clone());
            }
            retained.provenance.sort();
            retained.provenance.dedup();
            ContentChange {
                target,
                kind: if retained.provenance == before && !adds_context {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::Metadata
                },
            }
        } else {
            if catalog.items.contains_key(&item.target) {
                return Err(ChessError::IdentityChanged(item.target.clone()));
            }
            let mut created = item.clone();
            created.version = ContentVersion(1);
            catalog.items.insert(created.target.clone(), created);
            ContentChange {
                target: item.target.clone(),
                kind: ChangeKind::Added,
            }
        };
        index.insert(&catalog.items[&change.target], identity)?;
        changes.push(change);
    }
    catalog.validate()?;
    let alternate_solutions = captured_positions
        .iter()
        .filter(|position| {
            index
                .solutions_per_position
                .get(*position)
                .is_some_and(|&count| count > 1)
        })
        .count();
    Ok(TacticCapturePlan {
        content: ContentPlan { catalog, changes },
        alternate_solutions,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TacticIdentity {
    position: PositionKey,
    solution: Vec<String>,
}

impl TacticIdentity {
    fn from_item(item: &ContentItem) -> Option<Self> {
        let ContentKind::Tactic { practice_start } = &item.definition.kind else {
            return None;
        };
        Some(Self {
            position: practice_start.clone(),
            solution: item
                .definition
                .decisions
                .iter()
                .flat_map(|decision| std::iter::once(&decision.expected).chain(&decision.autoplay))
                .map(|answer| answer.uci.clone())
                .collect(),
        })
    }
}

#[derive(Default)]
struct CaptureIndex {
    by_target: BTreeMap<TargetId, TacticIdentity>,
    by_source: BTreeMap<String, TargetId>,
    by_identity: BTreeMap<TacticIdentity, TargetId>,
    solutions_per_position: BTreeMap<PositionKey, usize>,
}

impl CaptureIndex {
    fn insert(&mut self, item: &ContentItem, identity: TacticIdentity) -> Result<(), ChessError> {
        use std::collections::btree_map::Entry;
        self.by_target.insert(item.target.clone(), identity.clone());
        for token in source_tokens(item) {
            match self.by_source.entry(token.to_owned()) {
                Entry::Occupied(owner) => {
                    if self.by_target[owner.get()] != identity {
                        return Err(source_collision(token));
                    }
                }
                Entry::Vacant(owner) => {
                    owner.insert(item.target.clone());
                }
            }
        }
        if let Entry::Vacant(entry) = self.by_identity.entry(identity.clone()) {
            *self
                .solutions_per_position
                .entry(identity.position.clone())
                .or_default() += 1;
            entry.insert(item.target.clone());
        }
        Ok(())
    }

    fn find(
        &self,
        item: &ContentItem,
        identity: &TacticIdentity,
    ) -> Result<Option<TargetId>, ChessError> {
        let mut owner = None;
        for token in source_tokens(item) {
            if let Some(target) = self.by_source.get(token) {
                if &self.by_target[target] != identity {
                    return Err(source_collision(token));
                }
                owner.get_or_insert(target);
            }
        }
        if let Some(own) = self.by_target.get(&item.target) {
            if own != identity {
                return Err(ChessError::IdentityChanged(item.target.clone()));
            }
            owner = Some(&item.target);
        }
        Ok(owner.or_else(|| self.by_identity.get(identity)).cloned())
    }
}

fn source_tokens(item: &ContentItem) -> impl Iterator<Item = &str> {
    std::iter::once(item.target.0.as_str())
        .chain(item.provenance.iter().map(String::as_str))
        .filter(|token| is_source_token(token))
}

fn source_collision(token: &str) -> ChessError {
    let decode = |value: &str| {
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&value[index..index + 2], 16).expect("source tokens are hex")
            })
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    // This token came from source_tokens, which recognizes only framed hex.
    let pair = token
        .strip_prefix("tactic:")
        .and_then(|pair| pair.split_once(':'));
    let (namespace, id) = pair.expect("source token has two components");
    invalid(format!(
        "tactic source {}/{} identifies a different position or solution",
        decode(namespace),
        decode(id)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview_tactic_pgn;

    fn capture(source_id: &str, clocks: &str, solution: &str) -> Catalog {
        let pgn = format!(
            "[TempoSource \"Example provider\"]\n[TempoSourceId \"{source_id}\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - {clocks}\"]\n{solution} *",
        );
        preview_tactic_pgn(&pgn, &[], &format!("archive:{source_id}"))
            .unwrap()
            .selected_catalog(&[0])
            .unwrap()
    }

    #[test]
    fn equivalence_retains_identity_and_adds_source_provenance_without_revising_content() {
        let current = capture("old-id", "0 1", "e4 Kf7");
        let original = current.items.values().next().unwrap();
        let incoming = capture("new-id", "12 62", "e4 Kf7");
        let alias = incoming.items.keys().next().unwrap();
        let plan = plan_tactic_capture(&current, &incoming).unwrap();
        let matched = &plan.content.catalog.items[&original.target];
        assert_eq!(plan.content.catalog.items.len(), 1);
        assert_eq!(matched.version, original.version);
        assert_eq!(matched.definition, original.definition);
        assert!(matched.provenance.contains(&alias.0));
        assert_eq!(plan.content.changes[0].kind, ChangeKind::Metadata);
        assert_eq!(plan.alternate_solutions, 0);
        let repeated = plan_tactic_capture(&plan.content.catalog, &incoming).unwrap();
        assert_eq!(repeated.content.catalog, plan.content.catalog);
        assert_eq!(repeated.content.changes[0].kind, ChangeKind::Unchanged);
    }

    #[test]
    fn different_final_replies_are_distinct_and_authoritative_source_collisions_fail() {
        let old = capture("first", "0 1", "e4 Kf7");
        let new = capture("second", "0 1", "e4 Ke7");
        let plan = plan_tactic_capture(&old, &new).unwrap();
        assert_eq!(plan.content.catalog.items.len(), 2);
        assert_eq!(plan.alternate_solutions, 1);
        assert!(plan.content.catalog.items.values().all(ContentItem::active));
        let collision = capture("first", "0 1", "e4 Ke7");
        let error = plan_tactic_capture(&plan.content.catalog, &collision).unwrap_err();
        assert!(error.to_string().contains("Example provider/first"));
    }
}
