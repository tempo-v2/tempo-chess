//! Draft source-preserving repertoire edits from explicitly marked legal lines.
use crate::parsed_pgn::{MAX_PLIES, invalid, parse_pgn};
use crate::pgn_tokens::{PgnTokens, Token};
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::standard_uci::legal_move;
use crate::{AuthoredRepertoire, ChessError, LearnerColor, PositionKey};
use cozy_chess::util::display_san_move;
use cozy_chess::{Board, Color};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Ord, PartialOrd, Eq, PartialEq)]
pub struct MoveReplacement {
    pub route: String,
    pub old_move: String,
    pub new_move: String,
}

pub struct MarkedDraft {
    pub pgn: String,
    pub replacements: BTreeSet<MoveReplacement>,
    pub retired_routes: BTreeSet<String>,
}

fn san_line(mut board: Board, moves: &[String]) -> Result<String, ChessError> {
    let mut result = Vec::new();
    for uci in moves {
        let mv = legal_move(&board, uci)?;
        let san = display_san_move(&board, mv);
        let number = board.fullmove_number();
        result.push(match board.side_to_move() {
            Color::White => format!("{number}. {san}"),
            Color::Black if result.is_empty() => format!("{number}... {san}"),
            _ => san.to_string(),
        });
        board.play_unchecked(mv);
    }
    Ok(result.join(" "))
}

fn anchor_prefix(
    book: &AuthoredRepertoire,
    anchor: &PositionKey,
    route: Option<&[String]>,
    color: LearnerColor,
) -> Result<Vec<String>, ChessError> {
    let start = Board::default();
    if let Some(route) = route {
        let mut board = start;
        for (index, uci) in route.iter().enumerate() {
            let key = board_key(&board);
            let node = book.positions.get(&key);
            let in_book = node.is_some_and(|node| node.replies.contains_key(uci));
            // A response to a game gap starts just after an uncovered opponent move.
            let gap = index + 1 == route.len()
                && node.is_some()
                && board.side_to_move() != color.board_color();
            if !in_book && !gap {
                return Err(invalid(
                    "The starting route is outside the selected repertoire.",
                ));
            }
            let mv = legal_move(&board, uci)?;
            board.play_unchecked(mv);
        }
        if board_key(&board) != *anchor {
            return Err(invalid(
                "The starting route does not reach the analysis board.",
            ));
        }
        return Ok(route.to_vec());
    }
    if board_key(&start) == *anchor {
        return Ok(Vec::new());
    }
    book.positions
        .get(anchor)
        .map(|node| node.prefix.clone())
        .ok_or_else(|| invalid("Choose an authored position or a verified game-gap route."))
}

/// Build the exact PGN to submit through signed Preview/Save. Canonical learner positions
/// share one prescription; marked opponent alternatives can coexist.
pub fn draft_marked_lines(
    source: Option<&str>,
    color: LearnerColor,
    fen: &str,
    route: Option<&[String]>,
    lines: &[Vec<String>],
) -> Result<MarkedDraft, ChessError> {
    let board = parse_position(fen, true)?;
    let book = source
        .map(AuthoredRepertoire::parse)
        .transpose()?
        .unwrap_or_default();
    let prefix = anchor_prefix(&book, &board_key(&board), route, color)?;
    if lines.is_empty()
        || lines
            .iter()
            .any(|line| line.is_empty() || prefix.len() + line.len() > MAX_PLIES)
    {
        return Err(invalid(
            "Mark a nonempty legal line within the PGN length limit.",
        ));
    }
    let mut prescriptions = BTreeMap::<PositionKey, String>::new();
    let mut continuations = BTreeMap::<PositionKey, BTreeSet<Vec<String>>>::new();
    let mut additions = BTreeSet::new();
    for line in lines {
        let mut current = board.clone();
        for (i, uci) in line.iter().enumerate() {
            let mv = legal_move(&current, uci)?;
            if current.side_to_move() == color.board_color() {
                let key = board_key(&current);
                let saved = prescriptions
                    .entry(key.clone())
                    .or_insert_with(|| uci.clone());
                if saved != uci {
                    return Err(invalid(format!(
                        "Marked lines prescribe both {saved} and {uci} at the same position. Choose one."
                    )));
                }
                if book
                    .positions
                    .get(&key)
                    .is_some_and(|node| !node.replies.is_empty() && !node.replies.contains_key(uci))
                {
                    continuations
                        .entry(key)
                        .or_default()
                        .insert(line[i..].to_vec());
                }
            }
            current.play_unchecked(mv);
        }
        let mut full = prefix.clone();
        full.extend_from_slice(line);
        additions.insert(full);
    }
    let mut draft = MarkedDraft {
        pgn: source.unwrap_or_default().into(),
        replacements: BTreeSet::new(),
        retired_routes: BTreeSet::new(),
    };
    if !continuations.is_empty() {
        rewrite(
            source.unwrap_or_default(),
            &continuations,
            &mut additions,
            &mut draft,
        )?;
    }
    let mut existing = BTreeSet::new();
    if !draft.pgn.is_empty() {
        for game in parse_pgn(&draft.pgn)? {
            for route in game.routes()? {
                existing.insert(
                    route
                        .into_iter()
                        .map(|i| game.plies[i].uci.clone())
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
    // Longer marked lines already include any marked prefix. A sorted prefix
    // range finds a matching continuation without scanning the whole book.
    for line in additions.iter().rev() {
        if existing
            .range(line.clone()..)
            .next()
            .is_some_and(|known| known.starts_with(line))
        {
            continue;
        }
        let san = san_line(Board::default(), line)?;
        draft.pgn.push_str(&format!(
            "\n\n[Event \"Authored line\"]\n[Result \"*\"]\n\n{san} *\n"
        ));
        existing.insert(line.clone());
    }
    Ok(draft)
}

fn rewrite(
    source: &str,
    continuations: &BTreeMap<PositionKey, BTreeSet<Vec<String>>>,
    additions: &mut BTreeSet<Vec<String>>,
    draft: &mut MarkedDraft,
) -> Result<(), ChessError> {
    let games = parse_pgn(source)?;
    for game in &games {
        for route in game.routes()? {
            let moves: Vec<_> = route.iter().map(|&i| game.plies[i].uci.clone()).collect();
            for (i, index) in route.iter().enumerate() {
                let ply = &game.plies[*index];
                let Some(tails) = continuations.get(&board_key(&ply.board)) else {
                    continue;
                };
                let new_move = &tails.first().expect("nonempty marked suffixes")[0];
                let mv = legal_move(&ply.board, new_move)?;
                draft.replacements.insert(MoveReplacement {
                    route: san_line(Board::default(), &moves[..i])?,
                    old_move: ply.san.clone(),
                    new_move: display_san_move(&ply.board, mv).to_string(),
                });
                draft
                    .retired_routes
                    .insert(san_line(Board::default(), &moves)?);
                for tail in tails {
                    let mut route = moves[..i].to_vec();
                    route.extend_from_slice(tail);
                    if route.len() > MAX_PLIES {
                        return Err(invalid("The transposed line exceeds the PGN length limit."));
                    }
                    additions.insert(route);
                }
                break;
            }
        }
    }
    // Splice the changed suffix of each textual branch. All bytes outside
    // those ranges (including tags, NAGs and comments) stay in the document.
    let mut tokens = PgnTokens::new(source);
    let mut plies = games.iter().flat_map(|game| &game.plies);
    let mut depth = 0usize;
    let mut pending = None;
    let mut result = String::new();
    let mut copied = 0;
    while let Some(token) = tokens.next()? {
        match token {
            Token::San(san) => {
                let ply = plies
                    .next()
                    .ok_or_else(|| invalid("PGN source does not match its moves"))?;
                if pending.is_some() {
                    continue;
                }
                let Some(tails) = continuations.get(&board_key(&ply.board)) else {
                    continue;
                };
                let tail = tails.first().expect("nonempty marked suffixes");
                let start = san.as_ptr() as usize - source.as_ptr() as usize;
                result.push_str(&source[copied..start]);
                // An existing move number may immediately precede the replaced SAN.
                let mut after = ply.board.clone();
                let mv = legal_move(&after, &tail[0])?;
                result.push_str(&display_san_move(&after, mv).to_string());
                after.play_unchecked(mv);
                if tail.len() > 1 {
                    result.push(' ');
                    result.push_str(&san_line(after, &tail[1..])?);
                }
                result.push(' ');
                pending = Some(depth);
            }
            Token::VariationStart => depth += 1,
            Token::VariationEnd => {
                if pending == Some(depth) {
                    copied = source.len() - tokens.remaining().len() - 1;
                    pending = None;
                }
                depth -= 1;
            }
            Token::Result(mark) if pending.is_some() => {
                copied = mark.as_ptr() as usize - source.as_ptr() as usize;
                pending = None;
            }
            _ => {}
        }
    }
    result.push_str(&source[copied..]);
    draft.pgn = result;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(moves: &str) -> Vec<String> {
        moves.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn replacement_preserves_other_routes_and_untouched_pgn() {
        let source = "[Event \"Original\"]\n1.Nf3! $1 Nf6 (1... d5 {keep me} 2.g3) 2.g3 g6 3.Bg2 Bg7 *\n\n1.Nf3 g6 2.g3 Nf6 3.Bg2 d5 *";
        let mut board = Board::default();
        for uci in line("g1f3 g8f6 g2g3 g7g6") {
            let mv = legal_move(&board, &uci).unwrap();
            board.play_unchecked(mv);
        }
        let draft = draft_marked_lines(
            Some(source),
            LearnerColor::White,
            &board.to_string(),
            None,
            &[line("d2d4 d7d5"), line("d2d4 f8g7")],
        )
        .unwrap();
        assert_eq!(draft.replacements.len(), 2);
        assert_eq!(draft.retired_routes.len(), 2);
        assert!(
            draft
                .pgn
                .starts_with("[Event \"Original\"]\n1.Nf3! $1 Nf6 (1... d5 {keep me} 2.g3)")
        );
        let compiled =
            crate::compile_repertoire(&draft.pgn, "test", "Test", LearnerColor::White, "edits")
                .unwrap();
        assert_eq!(compiled.lines.len(), 5);
    }

    #[test]
    fn conflicting_marks_rejected_and_existing_prefix_is_noop() {
        let fen = Board::default().to_string();
        assert!(
            draft_marked_lines(
                None,
                LearnerColor::White,
                &fen,
                None,
                &[line("e2e4"), line("d2d4")]
            )
            .is_err()
        );
        let source = "1.e4!? {comment} e5 *";
        assert_eq!(
            draft_marked_lines(
                Some(source),
                LearnerColor::White,
                &fen,
                None,
                &[line("e2e4")]
            )
            .unwrap()
            .pgn,
            source
        );
    }
}
