//! Plan the complete replacement of one repertoire without editing other content.

use crate::parsed_pgn::invalid;
use crate::plan_import::version_imported_item;
use crate::{
    Catalog, ChangeKind, ChessError, ContentChange, ContentKind, ContentPlan, ContentStatus,
};
use std::collections::BTreeSet;

/// Replace the active routes of one repertoire from its complete intended PGN.
///
/// Use [`crate::compile_repertoire`] to build the incoming subset. Targets
/// absent from those routes retire without losing their definition or version.
/// A changed prescribed answer gets a new content version. The application
/// owns resetting its learning and committing the plan.
pub fn plan_repertoire_replacement(
    current: &Catalog,
    incoming: &Catalog,
    repertoire_id: &str,
) -> Result<ContentPlan, ChessError> {
    incoming.validate()?;
    let referenced: BTreeSet<_> = incoming
        .lines
        .iter()
        .flat_map(|line| line.targets.iter())
        .collect();
    if referenced.is_empty()
        || incoming
            .lines
            .iter()
            .any(|line| line.repertoire != repertoire_id)
        || incoming.items.iter().any(|(target, item)| {
            !item.active()
                || !belongs_to(&item.definition.kind, repertoire_id)
                || !referenced.contains(target)
        })
    {
        return Err(invalid(
            "replacement must contain only routes for the selected repertoire",
        ));
    }

    let mut catalog = current.clone();
    let mut changes = Vec::new();
    for item in catalog.items.values_mut() {
        if belongs_to(&item.definition.kind, repertoire_id)
            && item.active()
            && !referenced.contains(&item.target)
        {
            item.status = ContentStatus::Retired;
            changes.push(ContentChange {
                target: item.target.clone(),
                kind: ChangeKind::Metadata,
            });
        }
    }
    for (target, incoming_item) in &incoming.items {
        let mut item = incoming_item.clone();
        let kind = version_imported_item(current.items.get(target), &mut item)?;
        catalog.items.insert(target.clone(), item);
        changes.push(ContentChange {
            target: target.clone(),
            kind,
        });
    }

    // Keep the other repertoires' sequence and the selected repertoire's
    // position in the route catalog. Identical reuploads remain exact no-ops.
    let first = catalog
        .lines
        .iter()
        .position(|line| line.repertoire == repertoire_id);
    let mut routes = incoming.lines.iter().cloned();
    let mut lines = Vec::new();
    for (index, line) in catalog.lines.into_iter().enumerate() {
        if first == Some(index) {
            lines.extend(routes.by_ref());
        }
        if line.repertoire != repertoire_id {
            lines.push(line);
        }
    }
    lines.extend(routes);
    catalog.lines = lines;
    catalog.validate()?;
    Ok(ContentPlan { catalog, changes })
}

fn belongs_to(kind: &ContentKind, repertoire_id: &str) -> bool {
    matches!(kind, ContentKind::Repertoire { repertoire, .. } if repertoire == repertoire_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LearnerColor, compile_repertoire};
    use tempo_types::ContentVersion;

    fn book(pgn: &str, name: &str) -> Catalog {
        compile_repertoire(pgn, "book", name, LearnerColor::White, "book.pgn").unwrap()
    }

    #[test]
    fn omitted_lines_retire_only_unreferenced_targets_and_reactivation_reuses_versions() {
        let original = book("1.e4 e5 (1...c5 2.Nf3) 2.Nf3 *", "Original");
        let other =
            compile_repertoire("1.d4 *", "other", "Other", LearnerColor::White, "other.pgn")
                .unwrap();
        let mut current = original.clone();
        current.items.extend(other.items.clone());
        current.lines.extend(other.lines.clone());
        let first = &original.lines[0].targets[0];
        let changed = &original.lines[0].targets[1];
        let retired = &original.lines[1].targets[1];
        let renamed = book("1.e4 e5 2.Bc4 *", "Renamed");
        let replaced = plan_repertoire_replacement(&current, &renamed, "book").unwrap();
        assert_eq!(replaced.catalog.items[first].version, ContentVersion(1));
        assert_eq!(replaced.catalog.items[first].label, "Renamed");
        assert_eq!(replaced.catalog.items[changed].version, ContentVersion(2));
        assert_eq!(
            replaced.catalog.items[retired].status,
            ContentStatus::Retired
        );
        assert_eq!(replaced.catalog.items[retired].version, ContentVersion(1));
        assert_eq!(
            replaced.catalog.lines,
            [renamed.lines[0].clone(), other.lines[0].clone()]
        );
        assert_eq!(
            replaced.catalog.items[other.items.keys().next().unwrap()],
            *other.items.values().next().unwrap()
        );
        let repeated = plan_repertoire_replacement(&replaced.catalog, &renamed, "book").unwrap();
        assert_eq!(repeated.catalog, replaced.catalog);
        assert!(
            repeated
                .changes
                .iter()
                .all(|change| change.kind == ChangeKind::Unchanged)
        );

        let restored = book("1.e4 e5 (1...c5 2.Nf3) 2.Bc4 *", "Renamed");
        let reactivated =
            plan_repertoire_replacement(&repeated.catalog, &restored, "book").unwrap();
        assert!(reactivated.catalog.items[retired].active());
        assert_eq!(
            reactivated.catalog.items[retired].version,
            ContentVersion(1)
        );
        assert_eq!(
            reactivated.catalog.items[changed].version,
            ContentVersion(2)
        );
    }

    #[test]
    fn a_previous_move_or_comment_metadata_change_does_not_change_the_answer_version() {
        let a = book("1.Nf3 Nf6 2.g3 g6 3.Bg2 {main route} *", "Book");
        let b = book("1.Nf3 g6 2.g3 Nf6 3.Bg2 {transposed} *", "Book");
        let shared = &a.lines[0].targets[2];
        let plan = plan_repertoire_replacement(&a, &b, "book").unwrap();
        assert_eq!(plan.catalog.items[shared].version, ContentVersion(1));
        let lesson = plan.catalog.items[shared].lesson.as_ref().unwrap();
        assert_eq!(lesson.comments, ["transposed"]);
        assert_eq!(lesson.previous_move.as_deref(), Some("g8f6"));
    }
}
