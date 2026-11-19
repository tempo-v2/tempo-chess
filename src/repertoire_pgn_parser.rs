//! Adapt the existing import request to standard-start repertoire compilation.

use crate::parsed_pgn::invalid;
use crate::{Catalog, ChessError, ContentParser, ImportRequest, compile_repertoire};

/// Adapt the standard-profile import port to a fixed repertoire destination.
/// The caller supplies a stable identifier and an independent display label.
/// Use [`compile_repertoire`] directly for signed daily repertoire operations.
#[derive(Clone, Debug)]
pub struct RepertoirePgnParser {
    repertoire_id: String,
    label: String,
}

impl RepertoirePgnParser {
    pub fn new(repertoire_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            repertoire_id: repertoire_id.into(),
            label: label.into(),
        }
    }
}

impl ContentParser for RepertoirePgnParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError> {
        if request.repertoire.as_deref() != Some(&self.repertoire_id) {
            return Err(invalid(
                "PGN import requires the configured repertoire identity",
            ));
        }
        let learner_color = request
            .learner_color
            .ok_or_else(|| invalid("PGN import requires a learner color"))?;
        compile_repertoire(
            &request.pgn,
            &self.repertoire_id,
            &self.label,
            learner_color,
            &request.source,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChangeKind, ContentKind, LearnerColor, PositionCanonicalizer,
        StandardPositionCanonicalizer, plan_import,
    };
    use tempo_types::ContentVersion;

    fn parser() -> RepertoirePgnParser {
        RepertoirePgnParser::new("my-book", "My openings")
    }

    fn white(pgn: &str) -> ImportRequest {
        ImportRequest {
            pgn: pgn.into(),
            source: "opening book".into(),
            repertoire: Some("my-book".into()),
            learner_color: Some(LearnerColor::White),
        }
    }

    #[test]
    fn creates_a_decision_per_white_move_with_legal_standard_uci() {
        let request = white("1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O *");
        let catalog = parser().parse(&request).unwrap();
        let decisions: Vec<_> = catalog.lines[0]
            .targets
            .iter()
            .map(|target| {
                let item = &catalog.items[target];
                assert_eq!(item.provenance, std::slice::from_ref(&request.source));
                let ContentKind::Repertoire { position, .. } = &item.definition.kind else {
                    panic!("repertoire item");
                };
                let decision = &item.definition.decisions[0];
                assert_eq!(
                    StandardPositionCanonicalizer
                        .canonicalize(&decision.fen)
                        .unwrap(),
                    *position
                );
                assert!(decision.autoplay.is_empty());
                assert!(decision.fen.ends_with(" 0 1"));
                decision.expected.uci.as_str()
            })
            .collect();
        assert_eq!(decisions, ["e2e4", "g1f3", "f1b5", "b5a4", "e1g1"]);
    }

    #[test]
    fn transposed_games_and_repeated_positions_share_targets() {
        let transposed = parser()
            .parse(&white(
                "1. Nf3 Nf6 2. g3 g6 3. Bg2 *\n1. Nf3 g6 2. g3 Nf6 3. Bg2 *",
            ))
            .unwrap();
        assert_eq!(transposed.items.len(), 4);
        assert_eq!(
            transposed.lines[0].targets[2],
            transposed.lines[1].targets[2]
        );
        assert_ne!(
            transposed.lines[0].targets[1],
            transposed.lines[1].targets[1]
        );
        let repetition = parser()
            .parse(&white("1. Nf3 Nf6 2. Ng1 Ng8 3. Nf3 *"))
            .unwrap();
        assert_eq!(repetition.items.len(), 2);
        assert_eq!(
            repetition.lines[0].targets[0],
            repetition.lines[0].targets[2]
        );
    }

    #[test]
    fn annotating_the_same_line_preserves_its_identity_and_content_version() {
        let original = parser().parse(&white("1. e4 e5 2. Nf3 *")).unwrap();
        let mut annotated =
            white("[Event \"annotated\"]\n[Result \"*\"]\n1.e4! 1...e5 $2 {the reply} 2.Nf3?! *");
        annotated.source = "notes".into();
        let incoming = RepertoirePgnParser::new("my-book", "Renamed")
            .parse(&annotated)
            .unwrap();
        assert_eq!(incoming.lines, original.lines);
        assert_eq!(
            incoming.items.keys().collect::<Vec<_>>(),
            original.items.keys().collect::<Vec<_>>()
        );
        assert!(incoming.items.values().all(|item| item.label == "Renamed"));
        let plan = plan_import(&original, &incoming).unwrap();
        assert!(plan.changes.iter().all(|c| c.kind == ChangeKind::Metadata));
        assert!(
            plan.catalog
                .items
                .values()
                .all(|item| item.version == ContentVersion(1))
        );

        let replacement = parser().parse(&white("1. d4 *")).unwrap();
        let plan = plan_import(&plan.catalog, &replacement).unwrap();
        let target = &original.lines[0].targets[0];
        assert_eq!(plan.catalog.items[target].version, ContentVersion(2));
        assert_eq!(
            plan.catalog.items[target].definition.decisions[0]
                .expected
                .uci,
            "d2d4"
        );
    }

    #[test]
    fn rejects_bad_or_unsupported_input_as_a_whole() {
        for input in [
            "1. e4 e7 *",
            "1. e4+ *",
            "1. e4",
            "1. e4 (1. d4 *",
            "1. e4 *\n{unclosed",
            "1. e4 4...e5 *",
            "[Event \"unterminated]\n1.e4 *",
            "[Result \"1-0\"]\n1. e4 *",
            "[Variant \"Chess960\"]\n1. e4 *",
            "[SetUp \"1\"]\n1. e4 *",
            "[FEN \"irrelevant\"]\n1. e4 *",
            "*",
        ] {
            assert!(
                matches!(
                    parser().parse(&white(input)),
                    Err(ChessError::InvalidContent(_))
                ),
                "{input}"
            );
        }
        assert!(matches!(
            parser().parse(&white("1. e4 *\n1. d4 *")),
            Err(ChessError::ConflictingResponse { .. })
        ));
        let mut request = white("1. e4 *");
        request.learner_color = Some(LearnerColor::Black);
        assert!(parser().parse(&request).is_err());
        request.learner_color = None;
        assert!(parser().parse(&request).is_err());
        request = white("1. e4 *");
        request.repertoire = Some("My openings".into());
        assert!(parser().parse(&request).is_err());
    }

    #[test]
    fn imports_large_multi_game_pgns() {
        let game = format!("1. e4 {{ {} }} e5 2. Nf3 *\n", "a".repeat(1_024));
        let catalog = parser().parse(&white(&game.repeat(300))).unwrap();
        assert_eq!(catalog.items.len(), 2);
        assert_eq!(catalog.lines.len(), 1);
    }

    #[test]
    fn imports_a_long_mainline_with_deep_variations_and_large_route_expansion() {
        let plies = "Nf3 Nf6 Ng1 Ng8 ".repeat(130);
        let branches = "e5 (".repeat(75);
        let closes = ")".repeat(75);
        let pgn = format!("{plies}Nf3 {branches}e5{closes} *");
        let catalog = parser().parse(&white(&pgn)).unwrap();
        assert_eq!(catalog.lines.len(), 1);
        assert_eq!(catalog.lines[0].targets.len(), 261);

        let invalid = pgn.replacen("e5)", "e4)", 1);
        assert!(matches!(
            parser().parse(&white(&invalid)),
            Err(ChessError::InvalidContent(_))
        ));
    }
}
