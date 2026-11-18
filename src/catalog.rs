//! Catalog and its usage scenarios.

use crate::{ChessError, ContentItem, ContentKind, RepertoireLine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tempo_types::TargetId;

/// The complete set of content currently available to the application.
///
/// # When to use
///
/// Use a catalog when importing content, resolving a selected target, or comparing
/// played games with a repertoire. Parsers produce proposed catalogs; the
/// application and [`crate::plan_import`] consume them. A catalog contains no
/// attempts or learning schedules.
///
/// A [`ContentItem`] is one versioned target inside this collection. Its
/// [`crate::ChessDefinition`] is only the material needed to present and grade that
/// target. `validate()` checks collection-wide identities and line references as
/// well as each definition's shape; it does not validate chess legality.
///
/// ```
/// use tempo_chess::demo_catalog;
/// use tempo_types::TargetId;
///
/// let catalog = demo_catalog();
/// catalog.validate()?;
/// let target = TargetId::from("r1:p1");
/// let item = &catalog.items[&target];
/// assert!(item.active());
/// let exercise = item.exercise(); // The application can hand this to Study.
/// assert_eq!(exercise.target, target);
/// # Ok::<(), tempo_chess::ChessError>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub items: BTreeMap<TargetId, ContentItem>,
    pub lines: Vec<RepertoireLine>,
}
impl Catalog {
    pub fn validate(&self) -> Result<(), ChessError> {
        let mut decisions = BTreeMap::new();
        for (key, item) in &self.items {
            if key != &item.target || key.0.is_empty() || !key.0.is_ascii() || item.version.0 == 0 {
                return Err(ChessError::InvalidContent(
                    "target identity/version is invalid".into(),
                ));
            }
            item.definition.validate()?;
            match &item.definition.kind {
                ContentKind::Repertoire {
                    repertoire,
                    position,
                } => {
                    if repertoire.is_empty() || position.0.is_empty() {
                        return Err(ChessError::InvalidContent(
                            "repertoire and position identity required".into(),
                        ));
                    }
                    if decisions.insert((repertoire, position), key).is_some() {
                        return Err(ChessError::ConflictingResponse {
                            repertoire: repertoire.clone(),
                            position: position.clone(),
                        });
                    }
                }
                ContentKind::Tactic { practice_start } if practice_start.0.is_empty() => {
                    return Err(ChessError::InvalidContent(
                        "tactic start identity required".into(),
                    ));
                }
                ContentKind::Tactic { .. } => {}
            }
        }
        let mut line_ids = BTreeSet::new();
        for line in &self.lines {
            if line.id.is_empty() || !line_ids.insert(&line.id) {
                return Err(ChessError::InvalidContent(
                    "line identifiers must be unique and nonempty".into(),
                ));
            }
            for target in &line.targets {
                let item = self
                    .items
                    .get(target)
                    .ok_or_else(|| ChessError::UnknownTarget(target.clone()))?;
                if !matches!(&item.definition.kind, ContentKind::Repertoire { repertoire, .. } if repertoire == &line.repertoire)
                {
                    return Err(ChessError::InvalidContent(
                        "line target belongs to another repertoire".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo_catalog, plan_import};

    #[test]
    fn repetitions_share_target_and_conflicting_second_target_is_rejected() {
        let mut current = demo_catalog();
        current.lines[0].targets.push("r1:p1".into());
        current.validate().unwrap();
        let mut duplicate = current.items[&TargetId::from("r1:p1")].clone();
        duplicate.target = "r1:competing".into();
        duplicate.definition.decisions[0].expected = "d2d4".into();
        let incoming = Catalog {
            items: [(duplicate.target.clone(), duplicate)].into(),
            lines: vec![],
        };
        assert!(matches!(
            plan_import(&current, &incoming),
            Err(ChessError::ConflictingResponse { .. })
        ));
    }
}
