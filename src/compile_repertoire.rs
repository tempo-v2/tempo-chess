//! Compile a standard-start PGN into one repertoire's targets and routes.

use crate::parsed_pgn::{PgnGame, invalid, parse_pgn};
use crate::standard_position_canonicalizer::board_key;
use crate::{
    Catalog, ChessDefinition, ChessError, ContentItem, ContentKind, ContentStatus, Decision,
    GradingScheme, LearnerColor, RepertoireLesson, RepertoireLine,
};
use std::collections::{BTreeSet, btree_map::Entry};
use tempo_types::{ContentVersion, TargetId};

/// Compile all legal standard-start games and nested variations in a PGN.
///
/// Choose a stable repertoire identifier and a display label independently. A
/// position has one learner response within that repertoire. Multiple legal
/// routes can reach the same target, including repeated occurrences on one route.
/// The returned catalog contains only this import's repertoire.
pub fn compile_repertoire(
    pgn: &str,
    repertoire_id: &str,
    label: &str,
    learner_color: LearnerColor,
    source: &str,
) -> Result<Catalog, ChessError> {
    if repertoire_id.trim().is_empty() || label.trim().is_empty() {
        return Err(invalid(
            "PGN import requires a repertoire identity and name",
        ));
    }
    let escaped_repertoire = repertoire_id.escape_default().to_string();
    let identity = format!("pgn-v1:{}:{escaped_repertoire}:", escaped_repertoire.len());
    let import = RepertoireImport {
        repertoire_id,
        label,
        learner_color,
        source,
        identity,
    };
    let mut catalog = Catalog::default();
    let mut line_ids = BTreeSet::new();
    for game in parse_pgn(pgn)? {
        if game.tags.contains_key("FEN") {
            return Err(invalid(
                "repertoire PGN requires the standard initial position",
            ));
        }
        import.compile_game(&game, &mut catalog, &mut line_ids)?;
    }
    if catalog.items.is_empty() {
        return Err(invalid(
            "PGN contains no learner moves for the selected color",
        ));
    }
    catalog.validate()?;
    Ok(catalog)
}

struct RepertoireImport<'a> {
    repertoire_id: &'a str,
    label: &'a str,
    learner_color: LearnerColor,
    source: &'a str,
    identity: String,
}

impl RepertoireImport<'_> {
    fn compile_game(
        &self,
        game: &PgnGame<'_>,
        catalog: &mut Catalog,
        line_ids: &mut BTreeSet<String>,
    ) -> Result<(), ChessError> {
        let mut targets = Vec::with_capacity(game.plies.len());
        for (index, ply) in game.plies.iter().enumerate() {
            let target = if ply.board.side_to_move() == self.learner_color.board_color() {
                Some(self.compile_target(game, index, catalog)?)
            } else {
                None
            };
            targets.push(target);
        }
        for route in game.routes() {
            let mut id = format!("{}moves", self.identity);
            let mut occurrences = Vec::new();
            for index in route {
                id.push('|');
                id.push_str(&game.plies[index].uci);
                occurrences.extend(targets[index].iter().cloned());
            }
            if line_ids.insert(id.clone()) {
                catalog.lines.push(RepertoireLine {
                    id,
                    repertoire: self.repertoire_id.to_owned(),
                    targets: occurrences,
                });
            }
        }
        Ok(())
    }

    fn compile_target(
        &self,
        game: &PgnGame<'_>,
        index: usize,
        catalog: &mut Catalog,
    ) -> Result<TargetId, ChessError> {
        let ply = &game.plies[index];
        let position = board_key(&ply.board);
        let target = TargetId(format!("{}{}", self.identity, position.0));
        match catalog.items.entry(target.clone()) {
            Entry::Vacant(entry) => {
                let fen = format!("{} 0 1", position.0);
                entry.insert(ContentItem {
                    target: target.clone(),
                    version: ContentVersion(1),
                    label: self.label.to_owned(),
                    status: ContentStatus::Active,
                    definition: ChessDefinition {
                        scheme: GradingScheme::ExactSequenceV1,
                        kind: ContentKind::Repertoire {
                            repertoire: self.repertoire_id.to_owned(),
                            position,
                        },
                        decisions: vec![Decision {
                            fen,
                            expected: ply.uci.as_str().into(),
                            autoplay: vec![],
                        }],
                    },
                    provenance: vec![self.source.to_owned()],
                    opponent_lead_in: crate::OpponentLeadIn::from_pgn(game, index),
                    lesson: Some(RepertoireLesson {
                        san: ply.san.clone(),
                        comments: ply.comments.clone(),
                        previous_move: ply.previous.map(|prior| game.plies[prior].uci.clone()),
                    }),
                });
            }
            Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                if existing.definition.decisions[0].expected.uci != ply.uci {
                    let old = &existing.definition.decisions[0].expected.uci;
                    return Err(ChessError::ConflictingResponse {
                        repertoire: format!("{} ({old} and {})", self.label, ply.uci),
                        position,
                    });
                }
                if let Some(lesson) = &mut existing.lesson {
                    for comment in &ply.comments {
                        if !lesson.comments.contains(comment) {
                            lesson.comments.push(comment.clone());
                        }
                    }
                }
            }
        }
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_opponent_variations_keep_legal_context_and_shared_targets() {
        let catalog = compile_repertoire(
            "{Open games} 1.e4 {occupy the center} e5 (1...c5 (1...e6) 2.Nf3) 2.Nf3 *",
            "rapier",
            "White book",
            LearnerColor::White,
            "book",
        )
        .unwrap();
        let [main, sicilian, french] = catalog.lines.as_slice() else {
            panic!("expected three routes");
        };
        assert_eq!(main.targets.len(), 2);
        assert_eq!(sicilian.targets.len(), 2);
        assert_eq!(french.targets.len(), 1);
        assert_eq!(main.targets[0], sicilian.targets[0]);
        assert_eq!(main.targets[0], french.targets[0]);
        assert_ne!(main.targets[1], sicilian.targets[1]);
        assert_eq!(catalog.items.len(), 3);
        let first_lesson = catalog.items[&main.targets[0]].lesson.as_ref().unwrap();
        assert_eq!(first_lesson.comments, ["Open games", "occupy the center"]);
        let lesson = catalog.items[&sicilian.targets[1]].lesson.as_ref().unwrap();
        assert_eq!(lesson.san, "Nf3");
        assert_eq!(lesson.previous_move.as_deref(), Some("c7c5"));
    }

    #[test]
    fn black_distinguishes_opponent_branches_and_rejects_alternative_responses() {
        let compile = |pgn| compile_repertoire(pgn, "black", "Book", LearnerColor::Black, "book");
        let catalog = compile("1.e4 (1.d4 Nf6) c5 *").unwrap();
        assert_eq!(catalog.lines.len(), 2);
        assert_eq!(catalog.items.len(), 2);
        assert!(matches!(
            compile("1.e4 c5 (1...e5) *"),
            Err(ChessError::ConflictingResponse { .. })
        ));
        assert!(compile("1.e4 c5 (1...e4) *").is_err());
    }

    #[test]
    fn rejects_a_nonstandard_repertoire_start() {
        let pgn = "[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n1.e4 *";
        let error =
            compile_repertoire(pgn, "black", "Book", LearnerColor::Black, "book").unwrap_err();
        assert!(error.to_string().contains("standard initial position"));
    }
}
