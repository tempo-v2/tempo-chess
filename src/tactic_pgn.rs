//! Preview legal puzzle snippets and explicit game segments from the shared PGN parser.

use crate::compile_tactics::compile_pgn_tactic;
use crate::parsed_pgn::{PgnGame, invalid, parse_pgn_records};
use crate::plan_tactic_capture::plan_tactic_items;
use crate::revealed_line::numbered_san;
use crate::standard_position_canonicalizer::board_key;
use crate::{
    Catalog, ChessError, ContentItem, LearnerColor, RevealedLine, RevealedMove, TacticRecord,
};
use cozy_chess::Color;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PREVIEW_PLIES: usize = 16_384;

/// Override a record's default interpretation or select a route and segment.
/// Record, route, and start offsets are zero-based. The end offset is exclusive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TacticPgnSelection {
    pub record: usize,
    pub route: Option<usize>,
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub setup_plies: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TacticPgnStatus {
    Valid,
    SelectionRequired,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TacticPgnMove {
    pub san: String,
    pub fen_before: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TacticPgnRoute {
    pub route: usize,
    pub moves: Vec<TacticPgnMove>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TacticPgnRow {
    pub record: usize,
    pub status: TacticPgnStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver_fen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learner_color: Option<LearnerColor>,
    /// Complete selected line in standard UCI, including any final opponent reply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution: Option<Vec<String>>,
    pub routes: Vec<TacticPgnRoute>,
    /// Stable equivalent compiled record, independent of selection/preview token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay: Option<RevealedLine>,
    #[serde(skip)]
    item: Option<ContentItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TacticPgnPreview {
    pub rows: Vec<TacticPgnRow>,
}

impl TacticPgnPreview {
    /// Compile only the explicitly accepted rows. Invalid rows and repeated
    /// row indices reject the entire selection.
    pub fn selected_catalog(&self, records: &[usize]) -> Result<Catalog, ChessError> {
        if records.is_empty() {
            return Err(invalid("select at least one valid PGN record to save"));
        }
        let distinct: BTreeSet<_> = records.iter().copied().collect();
        if distinct.len() != records.len() {
            return Err(invalid("a tactic record was selected more than once"));
        }
        let items = distinct
            .into_iter()
            .map(|index| {
                self.rows
                    .get(index)
                    .and_then(|row| row.item.as_ref())
                    .ok_or_else(|| {
                        invalid(format!(
                            "PGN record {} needs a valid puzzle selection",
                            index.saturating_add(1)
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        plan_tactic_items(&Catalog::default(), items).map(|plan| plan.content.catalog)
    }
}

/// Parse a bounded PGN batch, retaining individual syntax/legality errors.
/// A single-route FEN snippet defaults to its complete solver-first line.
/// Games and branched snippets require a selection.
pub fn preview_tactic_pgn(
    pgn: &str,
    selections: &[TacticPgnSelection],
    source: &str,
) -> Result<TacticPgnPreview, ChessError> {
    let records = parse_pgn_records(pgn)?;
    let mut by_index = BTreeMap::new();
    for selection in selections {
        if selection.record >= records.len() {
            return Err(invalid("selection refers to a missing PGN record"));
        }
        if by_index.insert(selection.record, selection).is_some() {
            return Err(invalid("a PGN record has more than one selection"));
        }
    }
    let mut budget = MAX_PREVIEW_PLIES;
    let rows = records
        .into_iter()
        .enumerate()
        .map(|(record, game)| match game {
            Ok(game) => preview_record(
                record,
                &game,
                by_index.get(&record).copied(),
                source,
                &mut budget,
            ),
            Err(error) => TacticPgnRow::invalid(record, error),
        })
        .collect();
    Ok(TacticPgnPreview { rows })
}

fn preview_record(
    record: usize,
    game: &PgnGame<'_>,
    selection: Option<&TacticPgnSelection>,
    source: &str,
    budget: &mut usize,
) -> TacticPgnRow {
    let routes = match game.routes() {
        Ok(routes) => routes,
        Err(error) => return TacticPgnRow::invalid(record, error),
    };
    let preview_plies: usize = routes.iter().map(Vec::len).sum();
    let Some(remaining) = budget.checked_sub(preview_plies) else {
        return TacticPgnRow::invalid(
            record,
            invalid("PGN variation preview exceeds 16,384 route plies. Split the upload."),
        );
    };
    *budget = remaining;
    let mut row = TacticPgnRow {
        record,
        status: TacticPgnStatus::SelectionRequired,
        message: None,
        label: Some(label(game, record)),
        solver_fen: None,
        learner_color: None,
        solution: None,
        capture_identity: None,
        replay: None,
        routes: routes
            .iter()
            .enumerate()
            .map(|(route, path)| TacticPgnRoute {
                route,
                moves: path
                    .iter()
                    .map(|&index| {
                        let ply = &game.plies[index];
                        TacticPgnMove {
                            san: ply.san.clone(),
                            fen_before: ply.board.to_string(),
                        }
                    })
                    .collect(),
            })
            .collect(),
        item: None,
    };

    match select_path(game, &routes, selection) {
        Ok(Some(path)) => {
            if let Err(error) = row.compile(game, path, source) {
                row.status = TacticPgnStatus::Invalid;
                row.message = Some(error.to_string());
            }
        }
        Ok(None) => {
            row.message = Some(
                if can_default_segment(game, selection.and_then(|entry| entry.setup_plies)) {
                    "Choose the intended variation."
                } else {
                    "Choose a starting position and ending move for the tactic."
                }
                .into(),
            )
        }
        Err(error) => {
            row.message = Some(error.to_string());
            row.status = TacticPgnStatus::Invalid;
        }
    }
    row
}

struct SelectedPath<'a> {
    indices: &'a [usize],
    used_setup: bool,
}

fn select_path<'a>(
    game: &PgnGame<'_>,
    routes: &'a [Vec<usize>],
    selection: Option<&TacticPgnSelection>,
) -> Result<Option<SelectedPath<'a>>, ChessError> {
    let fen_start = game.tags.contains_key("FEN");
    let start = selection.and_then(|entry| entry.start);
    let end = selection.and_then(|entry| entry.end);
    if start.is_some() != end.is_some() {
        return Err(invalid(
            "choose both a starting position and an ending move",
        ));
    }
    let setup_override = selection.and_then(|entry| entry.setup_plies);
    let setup = match setup_override {
        Some(n @ 0..=1) => usize::from(n),
        Some(_) => return Err(invalid("setup can be zero or one ply")),
        None => match game.tags.get("TempoSetupPlies").copied() {
            None | Some("0") => 0,
            Some("1") => 1,
            Some(_) => return Err(invalid("TempoSetupPlies must be 0 or 1")),
        },
    };
    if !fen_start && setup != 0 {
        return Err(invalid(
            "setup plies apply to FEN snippets. Select the solver position in a game.",
        ));
    }
    if !can_default_segment(game, setup_override) && start.is_none() {
        return Ok(None);
    }

    let route = match selection.and_then(|entry| entry.route) {
        Some(index) => routes
            .get(index)
            .ok_or_else(|| invalid("selected variation is not in the PGN"))?,
        None if routes.len() == 1 => &routes[0],
        None => return Ok(None),
    };
    let used_setup = start.is_none() && setup != 0;
    let start = start.unwrap_or(setup);
    let end = end.unwrap_or(route.len());
    let indices = route
        .get(start..end)
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| invalid("the selected variation and range must contain a solution move"))?;
    Ok(Some(SelectedPath {
        indices,
        used_setup,
    }))
}

fn can_default_segment(game: &PgnGame<'_>, setup_override: Option<u8>) -> bool {
    let recorded_game = game.result != "*"
        && ["White", "Black"]
            .into_iter()
            .all(|side| tag_text(game, side).is_some());
    game.tags.contains_key("FEN")
        && (!recorded_game || setup_override.is_some() || game.tags.contains_key("TempoSetupPlies"))
}

impl TacticPgnRow {
    fn invalid(record: usize, error: ChessError) -> Self {
        Self {
            record,
            status: TacticPgnStatus::Invalid,
            message: Some(error.to_string()),
            label: None,
            solver_fen: None,
            learner_color: None,
            solution: None,
            capture_identity: None,
            replay: None,
            routes: Vec::new(),
            item: None,
        }
    }

    fn compile(
        &mut self,
        game: &PgnGame<'_>,
        path: SelectedPath<'_>,
        source: &str,
    ) -> Result<(), ChessError> {
        let start = &game.plies[path.indices[0]].board;
        let position = board_key(start);
        let solver_fen = format!("{} 0 1", position.0);
        let solution: Vec<_> = path
            .indices
            .iter()
            .map(|&index| game.plies[index].uci.clone())
            .collect();
        let (namespace, source_id) = source_identity(game, &position.0, &solution)?;
        let mut provenance = Vec::new();
        if !source.trim().is_empty() {
            provenance.push(source.to_owned());
        }
        if let Some(site) = tag_text(game, "Site") {
            provenance.push(format!("Site: {site}"));
        }
        let tactic = TacticRecord {
            namespace,
            source_id,
            solver_fen: solver_fen.clone(),
            solution: solution.clone(),
            label: self.label.clone().expect("parsed records have labels"),
            provenance,
        };
        let legal = path
            .indices
            .iter()
            .map(|&index| game.plies[index].mv)
            .collect::<Vec<_>>();
        let item = compile_pgn_tactic(&tactic, &legal)?;
        let mut hash = blake3::Hasher::new();
        for text in [&item.target.0, &tactic.label, &tactic.solver_fen]
            .into_iter()
            .chain(tactic.solution.iter())
            .chain(tactic.provenance.iter())
        {
            hash.update(&(text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
        self.capture_identity = Some(hash.finalize().to_hex().to_string());
        self.replay = Some(RevealedLine {
            start_fen: start.to_string(),
            moves: path
                .indices
                .iter()
                .map(|&index| {
                    let ply = &game.plies[index];
                    let mut after = ply.board.clone();
                    after.play(ply.mv);
                    RevealedMove {
                        notation: numbered_san(&ply.board, &ply.san),
                        fen: after.to_string(),
                    }
                })
                .collect(),
        });
        self.item = Some(item);
        self.status = TacticPgnStatus::Valid;
        self.solver_fen = Some(solver_fen);
        self.learner_color = Some(match start.side_to_move() {
            Color::White => LearnerColor::White,
            Color::Black => LearnerColor::Black,
        });
        self.solution = Some(solution);
        self.message = path
            .used_setup
            .then(|| "The first PGN move is used as setup.".into());
        Ok(())
    }
}

fn source_identity(
    game: &PgnGame<'_>,
    canonical_fen: &str,
    solution: &[String],
) -> Result<(String, String), ChessError> {
    match (
        tag_text(game, "TempoSource"),
        tag_text(game, "TempoSourceId"),
    ) {
        (Some(namespace), Some(id))
            if namespace != "tempo-pgn" && namespace.len() <= 120 && id.len() <= 300 =>
        {
            Ok((namespace, id))
        }
        (None, None)
            if !game.tags.contains_key("TempoSource")
                && !game.tags.contains_key("TempoSourceId") =>
        {
            let mut hash = blake3::Hasher::new();
            hash.update(canonical_fen.as_bytes());
            for uci in solution {
                hash.update(b"|");
                hash.update(uci.as_bytes());
            }
            Ok(("tempo-pgn".into(), hash.finalize().to_hex().to_string()))
        }
        _ => Err(invalid(
            "provide a valid TempoSource and TempoSourceId together",
        )),
    }
}

fn tag_text(game: &PgnGame<'_>, name: &str) -> Option<String> {
    game.tags
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != "?")
        .map(|value| {
            // The lexer already checks that each escape is a backslash or quote.
            let mut chars = value.chars();
            let mut text = String::with_capacity(value.len());
            while let Some(ch) = chars.next() {
                text.push(if ch == '\\' {
                    chars.next().expect("valid tag escape")
                } else {
                    ch
                });
            }
            text
        })
}

fn label(game: &PgnGame<'_>, record: usize) -> String {
    tag_text(game, "TempoLabel")
        .or_else(|| tag_text(game, "Event"))
        .or_else(|| tag_text(game, "PuzzleID").map(|id| format!("Puzzle {id}")))
        .unwrap_or_else(|| format!("Tactic {}", record + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChessState, ContentKind, project_position};

    const SIMPLE_FEN: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

    fn snippet(fen: &str, moves: &str) -> String {
        format!("[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{moves}\n")
    }

    fn selection(record: usize) -> TacticPgnSelection {
        TacticPgnSelection {
            record,
            route: None,
            start: None,
            end: None,
            setup_plies: None,
        }
    }

    #[test]
    fn fen_records_preserve_legal_special_moves_and_a_final_reply() {
        let input = [
            snippet("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "1.O-O O-O-O *"),
            snippet("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "2.exd6 Kf7 *"),
            snippet("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "1.a8=N Ke7 *"),
        ]
        .join("\n");
        let report = preview_tactic_pgn(&input, &[], "pgn:example").unwrap();
        let catalog = report.selected_catalog(&[0, 1, 2]).unwrap();
        for (row, solution) in
            report
                .rows
                .iter()
                .zip([["e1g1", "e8c8"], ["e5d6", "e8f7"], ["a7a8n", "e8e7"]])
        {
            assert_eq!(row.status, TacticPgnStatus::Valid);
            assert_eq!(row.learner_color, Some(LearnerColor::White));
            assert_eq!(row.solution.as_ref().unwrap(), &solution);
        }
        for item in catalog.items.values() {
            assert!(matches!(item.definition.kind, ContentKind::Tactic { .. }));
            assert_eq!(item.definition.decisions[0].autoplay.len(), 1);
            let done = project_position(&item.definition, &ChessState { decision_index: 1 }, false)
                .unwrap();
            assert_eq!(done.learner_color, LearnerColor::White);
            assert!(done.display_fen.contains(" w "));
        }
    }

    #[test]
    fn setup_is_explicit_and_an_override_wins_over_the_pgn_tag() {
        let line = snippet(SIMPLE_FEN, "1.e4 Kf7 2.e5 *");
        let unrelated_puzzle_id = format!("[PuzzleID \"xyz\"]\n{line}");
        let generic = preview_tactic_pgn(&unrelated_puzzle_id, &[], "").unwrap();
        assert_eq!(generic.rows[0].solver_fen.as_deref(), Some(SIMPLE_FEN));
        assert_eq!(
            generic.rows[0].solution.as_ref().unwrap(),
            &["e2e4", "e8f7", "e4e5"]
        );

        let setup = format!("[TempoSetupPlies \"1\"]\n{line}");
        let parsed = preview_tactic_pgn(&setup, &[], "").unwrap();
        assert_eq!(parsed.rows[0].learner_color, Some(LearnerColor::Black));
        assert_eq!(parsed.rows[0].solution.as_ref().unwrap(), &["e8f7", "e4e5"]);

        let input = TacticPgnSelection {
            setup_plies: Some(0),
            ..selection(0)
        };
        let overridden = preview_tactic_pgn(&setup, &[input], "").unwrap();
        assert_eq!(overridden.rows[0].solver_fen.as_deref(), Some(SIMPLE_FEN));
    }

    #[test]
    fn an_explicit_game_segment_selects_the_board_for_one_variation() {
        let game = "1.e4 e5 (1...c5 2.Nf3 d6) 2.Nf3 Nc6 *";
        let report = preview_tactic_pgn(game, &[], "").unwrap();
        let row = &report.rows[0];
        assert_eq!(row.status, TacticPgnStatus::SelectionRequired);
        assert_eq!(row.routes.len(), 2);
        let target_board = &row.routes[1].moves[1].fen_before;
        assert_eq!(row.routes[1].moves[1].san, "c5");
        assert!(report.selected_catalog(&[0]).is_err());

        let interval = TacticPgnSelection {
            route: Some(1),
            start: Some(1),
            end: Some(4),
            ..selection(0)
        };
        let selected = preview_tactic_pgn(game, &[interval], "").unwrap();
        let row = &selected.rows[0];
        assert_eq!(row.learner_color, Some(LearnerColor::Black));
        assert_eq!(row.solution.as_ref().unwrap(), &["c7c5", "g1f3", "d7d6"]);
        assert_eq!(
            row.solver_fen
                .as_ref()
                .unwrap()
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>(),
            target_board.split_whitespace().take(3).collect::<Vec<_>>()
        );
        assert_eq!(selected.selected_catalog(&[0]).unwrap().items.len(), 1);
    }

    #[test]
    fn a_recorded_finished_game_with_a_fen_start_still_requires_a_segment() {
        let pgn = format!(
            "[White \"Player One\"]\n[Black \"Player Two\"]\n{}",
            snippet(SIMPLE_FEN, "1.e4 Kf7 2.e5 1-0"),
        );
        let report = preview_tactic_pgn(&pgn, &[], "").unwrap();
        assert_eq!(report.rows[0].status, TacticPgnStatus::SelectionRequired);
        let interval = TacticPgnSelection {
            start: Some(1),
            end: Some(3),
            ..selection(0)
        };
        let selected = preview_tactic_pgn(&pgn, &[interval], "").unwrap();
        assert_eq!(selected.rows[0].learner_color, Some(LearnerColor::Black));
        assert_eq!(
            selected.rows[0].solution.as_ref().unwrap(),
            &["e8f7", "e4e5"]
        );
    }

    #[test]
    fn mixed_inputs_keep_invalid_rows_and_save_only_distinct_valid_selections() {
        let mixed = [
            snippet(SIMPLE_FEN, "1.e5 *"),
            "1.e4 e5 *".into(),
            snippet(SIMPLE_FEN, "1.e4 *"),
            "[Event \"malformed\n1.e4 *".into(),
            snippet(SIMPLE_FEN, "1.e3 *"),
            "[SetUp \"1\"]\n1.e4 *".into(),
        ]
        .join("\n");
        let report = preview_tactic_pgn(&mixed, &[], "").unwrap();
        let statuses = report.rows.iter().map(|row| row.status).collect::<Vec<_>>();
        assert_eq!(
            statuses,
            [
                TacticPgnStatus::Invalid,
                TacticPgnStatus::SelectionRequired,
                TacticPgnStatus::Valid,
                TacticPgnStatus::Invalid,
                TacticPgnStatus::Valid,
                TacticPgnStatus::Invalid,
            ]
        );
        assert!(report.selected_catalog(&[2, 0]).is_err());
        assert!(report.selected_catalog(&[2, 2]).is_err());
        assert_eq!(report.selected_catalog(&[2, 4]).unwrap().items.len(), 2);
        let output = serde_json::to_value(&report.rows[0]).unwrap();
        assert_eq!(output["status"], "invalid");
        assert!(output.get("solver_fen").is_none());
    }
}
