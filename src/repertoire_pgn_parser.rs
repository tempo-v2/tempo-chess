//! Adapt the existing import request to standard-start repertoire compilation.

use crate::parsed_pgn::invalid;
use crate::{Catalog, ChessError, ContentParser, ImportRequest, compile_repertoire};

/// Parse standard-start PGN for the requested learner color. The existing import
/// interface uses its repertoire string as both identifier and label. Call
/// [`compile_repertoire`] to supply an identifier independently of its label.
#[derive(Clone, Copy, Debug, Default)]
pub struct RepertoirePgnParser;

impl ContentParser for RepertoirePgnParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError> {
        let repertoire = request
            .repertoire
            .as_deref()
            .ok_or_else(|| invalid("PGN import requires a repertoire name"))?;
        let learner_color = request
            .learner_color
            .ok_or_else(|| invalid("PGN import requires a learner color"))?;
        compile_repertoire(
            &request.pgn,
            repertoire,
            repertoire,
            learner_color,
            &request.source,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsed_pgn::{MAX_BYTES, MAX_GAMES, MAX_PLIES};
    use crate::{
        ChangeKind, ContentKind, LearnerColor, PositionCanonicalizer,
        StandardPositionCanonicalizer, plan_import,
    };
    use tempo_types::ContentVersion;

    fn white(pgn: &str) -> ImportRequest {
        ImportRequest {
            pgn: pgn.into(),
            source: "opening book".into(),
            repertoire: Some("My openings".into()),
            learner_color: Some(LearnerColor::White),
        }
    }

    #[test]
    fn creates_a_decision_per_white_move_with_legal_standard_uci() {
        let request = white("1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O *");
        let catalog = RepertoirePgnParser.parse(&request).unwrap();
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
        let transposed = RepertoirePgnParser
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
        let repetition = RepertoirePgnParser
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
        let original = RepertoirePgnParser
            .parse(&white("1. e4 e5 2. Nf3 *"))
            .unwrap();
        let mut annotated =
            white("[Event \"annotated\"]\n[Result \"*\"]\n1.e4! 1...e5 $2 {the reply} 2.Nf3?! *");
        annotated.source = "notes".into();
        let incoming = RepertoirePgnParser.parse(&annotated).unwrap();
        assert_eq!(incoming.lines, original.lines);
        let plan = plan_import(&original, &incoming).unwrap();
        assert!(plan.changes.iter().all(|c| c.kind == ChangeKind::Metadata));
        assert!(
            plan.catalog
                .items
                .values()
                .all(|item| item.version == ContentVersion(1))
        );

        let replacement = RepertoirePgnParser.parse(&white("1. d4 *")).unwrap();
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
                    RepertoirePgnParser.parse(&white(input)),
                    Err(ChessError::InvalidContent(_))
                ),
                "{input}"
            );
        }
        assert!(matches!(
            RepertoirePgnParser.parse(&white("1. e4 *\n1. d4 *")),
            Err(ChessError::ConflictingResponse { .. })
        ));
        let mut request = white("1. e4 *");
        request.learner_color = Some(LearnerColor::Black);
        assert!(RepertoirePgnParser.parse(&request).is_err());
        request.learner_color = None;
        assert!(RepertoirePgnParser.parse(&request).is_err());
        request = white("1. e4 *");
        request.repertoire = Some(" \t ".into());
        assert!(RepertoirePgnParser.parse(&request).is_err());
    }

    #[test]
    fn enforces_byte_game_and_ply_limits() {
        let games = "1.e4 *\n";
        assert!(
            RepertoirePgnParser
                .parse(&white(&games.repeat(MAX_GAMES)))
                .is_ok()
        );
        let plies = "Nf3 Nf6 Ng1 Ng8 ".repeat(MAX_PLIES / 4);
        assert!(
            RepertoirePgnParser
                .parse(&white(&format!("{plies}*")))
                .is_ok()
        );
        for input in [
            " ".repeat(MAX_BYTES + 1),
            games.repeat(MAX_GAMES + 1),
            format!("{plies}Nf3 *"),
        ] {
            assert!(matches!(
                RepertoirePgnParser.parse(&white(&input)),
                Err(ChessError::InvalidContent(_))
            ));
        }
    }
}
