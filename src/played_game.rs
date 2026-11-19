//! Played games use the legal mainline only, even when the PGN includes analysis.

use crate::parsed_pgn::{invalid, parse_pgn, parse_pgn_records};
use crate::revealed_line::{RevealedLine, RevealedMove, numbered_san};
use crate::standard_position_canonicalizer::board_key;
use crate::{AuthoredRepertoire, Catalog, ChessError, ContentKind, LearnerColor, PositionKey};
use std::collections::{BTreeMap, BTreeSet};
use tempo_types::{ContentVersion, TargetId};

#[derive(Debug)]
pub struct PlayedGame {
    pub tags: BTreeMap<String, String>,
    pub result: String,
    start: PositionKey,
    mainline_pgn: String,
    pub(super) moves: Vec<(PositionKey, cozy_chess::Color, String)>,
}

impl PlayedGame {
    pub fn mainline_pgn(&self) -> &str {
        &self.mainline_pgn
    }

    /// Stable played facts for local upload identity. Clocks and annotations are excluded.
    pub fn mainline_identity(&self) -> (&str, Vec<&str>) {
        (
            &self.start.0,
            self.moves.iter().map(|(_, _, uci)| uci.as_str()).collect(),
        )
    }
}

/// Decode completed orthodox games, retaining errors for separately framed records.
pub fn parse_played_games(pgn: &str) -> Result<Vec<Result<PlayedGame, ChessError>>, ChessError> {
    Ok(parse_pgn_records(pgn)?
        .into_iter()
        .map(|record| {
            let game = record?;
            if game.result == "*" {
                return Err(invalid("game is unfinished"));
            }
            let start = board_key(&game.plies[game.mainline[0]].board);
            let moves = game
                .mainline
                .iter()
                .map(|&index| {
                    let ply = &game.plies[index];
                    (
                        board_key(&ply.board),
                        ply.board.side_to_move(),
                        ply.uci.clone(),
                    )
                })
                .collect();
            let mut mainline_pgn = String::new();
            for &index in &game.mainline {
                let ply = &game.plies[index];
                mainline_pgn.push_str(&ply.san);
                mainline_pgn.push(' ');
            }
            mainline_pgn.push_str(game.result);
            Ok(PlayedGame {
                tags: game
                    .tags
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
                result: game.result.into(),
                start,
                mainline_pgn,
                moves,
            })
        })
        .collect())
}

/// Show an already played mainline through a verified deviation. The caller
/// supplies the retained observation and the issued repertoire position.
pub fn played_deviation(
    pgn: &str,
    ply: usize,
    position: &PositionKey,
    played: &str,
) -> Result<Option<(String, RevealedLine)>, ChessError> {
    let games = parse_pgn(pgn)?;
    let [game] = games.as_slice() else {
        return Ok(None);
    };
    let Some(last) = ply.checked_sub(1).and_then(|i| game.mainline.get(i)) else {
        return Ok(None);
    };
    let deviation = &game.plies[*last];
    if &board_key(&deviation.board) != position || deviation.uci != played {
        return Ok(None);
    }
    let moves = game.mainline[..ply]
        .iter()
        .map(|&index| {
            let ply = &game.plies[index];
            let notation = numbered_san(&ply.board, &ply.san);
            let mut after = ply.board.clone();
            after.play(ply.mv);
            RevealedMove {
                notation,
                fen: after.to_string(),
            }
        })
        .collect();
    Ok(Some((
        deviation.san.clone(),
        RevealedLine {
            start_fen: game.plies[game.mainline[0]].board.to_string(),
            moves,
        },
    )))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepertoireObservation {
    pub ply: usize,
    pub target: TargetId,
    pub version: ContentVersion,
    pub played: String,
    pub prescribed: String,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct RepertoireComparison {
    pub observations: Vec<RepertoireObservation>,
    pub coverage_gaps: usize,
    pub opponent_deviations: usize,
}

/// Compare the learner's actual moves with active targets from one repertoire.
/// A verified current authored document can establish known opponent branches.
pub fn compare_played_game(
    game: &PlayedGame,
    catalog: &Catalog,
    repertoire_id: &str,
    color: LearnerColor,
    current_document: Option<&str>,
) -> Result<RepertoireComparison, ChessError> {
    let mut targets = BTreeMap::new();
    for item in catalog.items.values().filter(|item| item.active()) {
        if let ContentKind::Repertoire {
            repertoire,
            position,
        } = &item.definition.kind
        {
            if repertoire == repertoire_id && targets.insert(position, item).is_some() {
                return Err(invalid(
                    "selected repertoire has ambiguous position coverage",
                ));
            }
        }
    }
    let mut opponent = BTreeMap::<PositionKey, BTreeSet<String>>::new();
    if let Some(pgn) = current_document {
        for authored_game in parse_pgn(pgn)? {
            for ply in authored_game.plies {
                if ply.board.side_to_move() != color.board_color() {
                    opponent
                        .entry(board_key(&ply.board))
                        .or_default()
                        .insert(ply.uci);
                }
            }
        }
    }

    let mut result = RepertoireComparison::default();
    for (index, (position, side, uci)) in game.moves.iter().enumerate() {
        if *side == color.board_color() {
            match targets.get(position) {
                Some(item) => result.observations.push(RepertoireObservation {
                    ply: index + 1,
                    target: item.target.clone(),
                    version: item.version,
                    played: uci.clone(),
                    prescribed: item.definition.decisions[0].expected.uci.clone(),
                }),
                None => result.coverage_gaps += 1,
            }
        } else {
            match opponent.get(position) {
                Some(replies) if !replies.contains(uci) => result.opponent_deviations += 1,
                Some(_) => {}
                None => result.coverage_gaps += 1,
            }
        }
    }
    Ok(result)
}

/// The combined result for one actual decision by the learner.
#[derive(Debug, PartialEq, Eq)]
pub enum DecisionAdherence {
    Matched,
    Miss {
        candidates: Vec<RepertoireObservation>,
    },
    Uncovered,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlayedDecision {
    pub ply: usize,
    pub position: PositionKey,
    pub previous_move: Option<String>,
    pub observations: Vec<RepertoireObservation>,
    pub adherence: DecisionAdherence,
}

/// Compare independently at each canonical position, including positions reached
/// by transposition. Authored routes disambiguate corrections, not adherence.
pub fn compare_active_repertoires(
    game: &PlayedGame,
    catalog: &Catalog,
    color: LearnerColor,
    documents: &BTreeMap<String, String>,
) -> Result<Vec<PlayedDecision>, ChessError> {
    let books = documents
        .iter()
        .map(|(id, pgn)| Ok((id.clone(), AuthoredRepertoire::parse(pgn)?)))
        .collect::<Result<_, ChessError>>()?;
    Ok(compare_authored_repertoires(game, catalog, color, &books))
}

/// Use the same compiled authored books across a batch of comparisons.
pub fn compare_authored_repertoires(
    game: &PlayedGame,
    catalog: &Catalog,
    color: LearnerColor,
    books: &BTreeMap<String, AuthoredRepertoire>,
) -> Vec<PlayedDecision> {
    let mut coverage = BTreeMap::<_, Vec<_>>::new();
    for item in catalog.items.values().filter(|item| item.active()) {
        if let ContentKind::Repertoire {
            repertoire,
            position,
        } = &item.definition.kind
        {
            coverage
                .entry(position)
                .or_default()
                .push((repertoire, item));
        }
    }

    game.moves
        .iter()
        .enumerate()
        .filter(|(_, (_, side, _))| *side == color.board_color())
        .map(|(index, (position, _, played))| {
            let covered = coverage.get(position).map_or(&[][..], Vec::as_slice);
            let observations: Vec<_> = covered
                .iter()
                .map(|(_, item)| RepertoireObservation {
                    ply: index + 1,
                    target: item.target.clone(),
                    version: item.version,
                    played: played.clone(),
                    prescribed: item.definition.decisions[0].expected.uci.clone(),
                })
                .collect();
            let adherence = if covered.is_empty() {
                DecisionAdherence::Uncovered
            } else if observations
                .iter()
                .any(|observation| observation.played == observation.prescribed)
            {
                DecisionAdherence::Matched
            } else {
                let distances: Vec<_> = covered
                    .iter()
                    .map(|(repertoire, _)| {
                        books.get(*repertoire).map(|book| {
                            book.routes
                                .iter()
                                .flat_map(|line| {
                                    line.iter()
                                        .enumerate()
                                        .filter(|(_, (key, _))| key == position)
                                        .map(|(end, _)| {
                                            line[..end]
                                                .iter()
                                                .rev()
                                                .zip(game.moves[..index].iter().rev())
                                                .take_while(|((key, uci), (actual, _, played))| {
                                                    key == actual && uci == played
                                                })
                                                .count()
                                        })
                                })
                                .max()
                                .unwrap_or(0)
                        })
                    })
                    .collect();
                let best = distances.iter().flatten().copied().max().unwrap_or(0);
                let candidates = observations
                    .iter()
                    .zip(distances)
                    .filter(|(_, score)| score.is_none_or(|value| value == best))
                    .map(|(observation, _)| observation.clone())
                    .collect();
                DecisionAdherence::Miss { candidates }
            };
            PlayedDecision {
                ply: index + 1,
                position: position.clone(),
                previous_move: index
                    .checked_sub(1)
                    .map(|previous| game.moves[previous].2.clone()),
                observations,
                adherence,
            }
        })
        .collect()
}

/// Count opposing moves that disagree with all verified authored branches
/// covering the position. Unknown branches do not establish disagreement.
pub fn opponent_deviations(
    game: &PlayedGame,
    color: LearnerColor,
    documents: &BTreeMap<String, String>,
) -> Result<usize, ChessError> {
    let mut replies = BTreeMap::<PositionKey, BTreeSet<String>>::new();
    for pgn in documents.values() {
        for authored in parse_pgn(pgn)? {
            for ply in authored.plies {
                if ply.board.side_to_move() != color.board_color() {
                    replies
                        .entry(board_key(&ply.board))
                        .or_default()
                        .insert(ply.uci);
                }
            }
        }
    }
    Ok(game
        .moves
        .iter()
        .filter(|(position, side, played)| {
            *side != color.board_color()
                && replies
                    .get(position)
                    .is_some_and(|moves| !moves.contains(played))
        })
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_repertoire;

    #[test]
    fn played_replay_uses_the_custom_start_and_stops_at_the_verified_mainline_move() {
        let pgn = r#"[SetUp "1"]
[FEN "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 9 14"]

14. Bc4 (14. Nf3) Nc6 1-0"#;
        let game = parse_pgn(pgn).unwrap().pop().unwrap();
        let position = board_key(&game.plies[game.mainline[0]].board);
        let (san, line) = played_deviation(pgn, 1, &position, "f1c4")
            .unwrap()
            .unwrap();
        assert_eq!(san, "Bc4");
        assert!(line.start_fen.ends_with(" 9 14"));
        assert_eq!(line.moves.len(), 1);
        assert_eq!(line.moves[0].notation, "14. Bc4");
        assert!(line.moves[0].fen.contains("/2B1P3/"));
        assert!(
            played_deviation(pgn, 1, &position, "g1f3")
                .unwrap()
                .is_none()
        );
        assert!(
            played_deviation(pgn, 2, &position, "b8c6")
                .unwrap()
                .is_none()
        );
        assert!(
            played_deviation(pgn, 3, &position, "b8c6")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn canonical_transposition_matches_even_after_uncovered_moves() {
        let book = "1. Nf3 Nf6 2. g3 g6 3. Bg2 *";
        let catalog =
            compile_repertoire(book, "book", "Book", LearnerColor::White, "source").unwrap();
        let game = parse_played_games("1. g3 g6 2. Nf3 Nf6 3. Bg2 1-0")
            .unwrap()
            .pop()
            .unwrap()
            .unwrap();
        let decisions = compare_active_repertoires(
            &game,
            &catalog,
            LearnerColor::White,
            &BTreeMap::from([("book".into(), book.into())]),
        )
        .unwrap();
        assert_eq!(decisions[1].adherence, DecisionAdherence::Uncovered);
        assert_eq!(decisions[2].adherence, DecisionAdherence::Matched);
    }

    #[test]
    fn compares_repeated_and_transposed_positions_within_selected_repertoire() {
        let book = "1. Nf3 Nf6 2. Ng1 Ng8 3. Nf3 g6 4. g3 Nf6 5. Bg2 *";
        let mut catalog =
            compile_repertoire(book, "selected", "Book", LearnerColor::White, "source").unwrap();
        catalog.items.extend(
            compile_repertoire("1. e4 *", "other", "Book", LearnerColor::White, "source")
                .unwrap()
                .items,
        );
        let game =
            parse_played_games("1. Nf3 (1. e4 e5) Nf6 2. Ng1 Ng8 3. Nf3 Nf6 4. g3 g6 5. d3 1-0")
                .unwrap()
                .pop()
                .unwrap()
                .unwrap();
        let result =
            compare_played_game(&game, &catalog, "selected", LearnerColor::White, Some(book))
                .unwrap();
        let observations = result.observations;
        assert_eq!(
            observations.iter().map(|obs| obs.ply).collect::<Vec<_>>(),
            [1, 3, 5, 7, 9]
        );
        assert_eq!(observations[0].target, observations[2].target);
        assert_eq!(observations[4].played, "d2d3");
        assert_eq!(observations[4].prescribed, "f1g2");
        assert_eq!(result.opponent_deviations, 0);
        assert!(result.coverage_gaps > 0);
    }

    #[test]
    fn black_comparison_skips_an_invalid_record_and_ignores_variations() {
        let book = "1. e4 e5 2. Nf3 Nc6 *";
        let catalog =
            compile_repertoire(book, "black", "Book", LearnerColor::Black, "source").unwrap();
        let mut records = parse_played_games("1. e9 1-0\n1.e4 e5 (1...c5) 2. Nf3 d6 0-1")
            .unwrap()
            .into_iter();
        assert!(records.next().unwrap().is_err());
        let result = compare_played_game(
            &records.next().unwrap().unwrap(),
            &catalog,
            "black",
            LearnerColor::Black,
            Some(book),
        )
        .unwrap();
        assert_eq!(result.observations.len(), 2);
        assert_eq!(result.observations[1].ply, 4);
        assert_eq!(result.observations[1].prescribed, "b8c6");
    }
}
