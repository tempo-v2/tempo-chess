//! Convert bounded orthodox PGN mainlines into a proposed repertoire catalog.

use crate::pgn_tokens::{PgnTokens, Token, is_result};
use crate::standard_position_canonicalizer::board_key;
use crate::{
    Catalog, ChessAnswer, ChessDefinition, ChessError, ContentItem, ContentKind, ContentParser,
    ContentStatus, Decision, GradingScheme, ImportRequest, RepertoireLine,
};
use cozy_chess::util::{display_san_move, display_uci_move, parse_san_move};
use cozy_chess::{Board, Color};
use std::collections::{BTreeSet, btree_map::Entry};
use tempo_types::{ContentVersion, TargetId};

const MAX_BYTES: usize = 256 * 1024;
const MAX_GAMES: usize = 32;
const MAX_PLIES: usize = 512;

/// Parse standard-start games and train the color specified in the request.
/// Each game contributes an ordered line of learner decisions. Opponent moves
/// establish later positions. Comments, annotations, and variations are skipped.
/// The caller can merge the resulting catalog with [`crate::plan_import`].
#[derive(Clone, Copy, Debug, Default)]
pub struct RepertoirePgnParser;

impl ContentParser for RepertoirePgnParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError> {
        if request.pgn.len() > MAX_BYTES {
            return Err(invalid("PGN exceeds 256 KiB"));
        }
        let repertoire = request
            .repertoire
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| invalid("PGN import requires a repertoire name"))?;
        let learner = request
            .learner_color
            .ok_or_else(|| invalid("PGN import requires a learner color"))?
            .board_color();
        let mut catalog = Catalog::default();
        let escaped_repertoire = repertoire.escape_default().to_string();
        let identity = format!("pgn-v1:{}:{escaped_repertoire}:", escaped_repertoire.len());
        let mut tokens = PgnTokens::new(&request.pgn);
        let mut line_ids = BTreeSet::new();

        for game_number in 1..=MAX_GAMES + 1 {
            let Some(first) = tokens.next()? else {
                break;
            };
            if game_number > MAX_GAMES {
                return Err(invalid("PGN exceeds 32 games"));
            }
            let game = parse_game(first, &mut tokens, learner)?;
            let mut targets = Vec::with_capacity(game.len());
            let mut line_id = identity.clone();
            for (board, answer) in game {
                let position = board_key(&board);
                let target = TargetId(format!("{identity}{}", position.0));
                // Orthodox FEN never contains '|'. The prefix frames the repertoire
                // name so distinct repertoires cannot produce the same identity.
                line_id.push_str(&position.0);
                line_id.push('|');
                match catalog.items.entry(target.clone()) {
                    Entry::Vacant(entry) => {
                        let fen = format!("{} 0 1", position.0);
                        entry.insert(ContentItem {
                            target: target.clone(),
                            version: ContentVersion(1),
                            label: repertoire.into(),
                            status: ContentStatus::Active,
                            definition: ChessDefinition {
                                scheme: GradingScheme::ExactSequenceV1,
                                kind: ContentKind::Repertoire {
                                    repertoire: repertoire.into(),
                                    position,
                                },
                                decisions: vec![Decision {
                                    fen,
                                    expected: answer,
                                    autoplay: vec![],
                                }],
                            },
                            provenance: vec![request.source.clone()],
                        });
                    }
                    Entry::Occupied(entry)
                        if entry.get().definition.decisions[0].expected != answer =>
                    {
                        return Err(ChessError::ConflictingResponse {
                            repertoire: repertoire.into(),
                            position,
                        });
                    }
                    Entry::Occupied(_) => {}
                }
                targets.push(target);
            }
            if line_ids.insert(line_id.clone()) {
                catalog.lines.push(RepertoireLine {
                    id: line_id,
                    repertoire: repertoire.into(),
                    targets,
                });
            }
        }
        if catalog.lines.is_empty() {
            return Err(invalid("PGN contains no games"));
        }
        catalog.validate()?;
        Ok(catalog)
    }
}

fn parse_game<'a>(
    first: Token<'a>,
    tokens: &mut PgnTokens<'a>,
    learner: Color,
) -> Result<Vec<(Board, ChessAnswer)>, ChessError> {
    let mut board = Board::default();
    let mut token = first;
    let mut tags = BTreeSet::new();
    let mut header_result = None;
    let mut numbered = false;
    let mut plies = 0;
    let mut decisions = vec![];
    loop {
        match token {
            Token::Tag(name, value) => {
                if plies != 0 || numbered || !tags.insert(name) {
                    return Err(invalid("duplicate or misplaced PGN tag"));
                }
                match (name, value) {
                    ("Variant", "Standard") | ("SetUp", "0") => {}
                    ("FEN" | "SetUp" | "Variant", _) => {
                        return Err(invalid(
                            "PGN requires the standard initial position and rules",
                        ));
                    }
                    ("Result", value) if is_result(value) => header_result = Some(value),
                    ("Result", _) => return Err(invalid("invalid Result tag")),
                    _ => {}
                }
            }
            Token::MoveNumber { number, black } => {
                if numbered
                    || number != board.fullmove_number()
                    || black != (board.side_to_move() == Color::Black)
                {
                    return Err(invalid("PGN move number does not match the mainline"));
                }
                numbered = true;
            }
            Token::San(san) => {
                plies += 1;
                if plies > MAX_PLIES {
                    return Err(invalid("PGN game exceeds 512 plies"));
                }
                let illegal =
                    || ChessError::InvalidContent(format!("invalid SAN at ply {plies}: {san}"));
                let mv = parse_san_move(&board, san).map_err(|_| illegal())?;
                if display_san_move(&board, mv).to_string() != san {
                    return Err(illegal());
                }
                if board.side_to_move() == learner {
                    decisions.push((
                        board.clone(),
                        ChessAnswer {
                            uci: display_uci_move(&board, mv).to_string(),
                        },
                    ));
                }
                board.try_play(mv).map_err(|_| illegal())?;
                numbered = false;
            }
            Token::Result(result) => {
                if numbered || header_result.is_some_and(|header| header != result) {
                    return Err(invalid("PGN result does not match the game"));
                }
                if decisions.is_empty() {
                    return Err(invalid("PGN game contains no learner moves"));
                }
                return Ok(decisions);
            }
        }
        token = tokens
            .next()?
            .ok_or_else(|| invalid("PGN game requires a result marker"))?;
    }
}

fn invalid(message: &str) -> ChessError {
    ChessError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChangeKind, LearnerColor, PositionCanonicalizer, StandardPositionCanonicalizer, plan_import,
    };

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
                assert_eq!(item.provenance, [request.source.clone()]);
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
        let mut annotated = white(
            "[Event \"annotated\"]\n[Result \"*\"]\n1.e4! (1. d4 d5 (1... Nf6)) 1...e5 $2 {the reply} 2.Nf3?! *",
        );
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
