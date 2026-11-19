//! Parse legal orthodox moves into a tree shared by content compilers.

use crate::ChessError;
use crate::pgn_tokens::{PgnTokens, Token, is_result};
use crate::standard_position_canonicalizer::parse_position;
use cozy_chess::util::{display_san_move, display_uci_move, parse_san_move};
use cozy_chess::{Board, Color, Move};
use std::collections::BTreeMap;

#[derive(Debug)]
pub(super) struct PgnPly {
    pub board: Board,
    pub san: String,
    pub uci: String,
    pub mv: Move,
    pub comments: Vec<String>,
    pub previous: Option<usize>,
    next: Vec<usize>,
}

#[derive(Debug)]
pub(super) struct PgnGame<'a> {
    pub tags: BTreeMap<&'a str, &'a str>,
    pub result: &'a str,
    roots: Vec<usize>,
    pub(super) mainline: Vec<usize>,
    pub plies: Vec<PgnPly>,
}

impl PgnGame<'_> {
    /// Enumerate finite root-to-leaf paths in PGN order (mainline first).
    pub fn routes(&self) -> impl Iterator<Item = Vec<usize>> + '_ {
        let mut branches = vec![self.roots.iter()];
        let mut path = Vec::new();
        std::iter::from_fn(move || {
            while let Some(branch) = branches.last_mut() {
                if let Some(&index) = branch.next() {
                    path.push(index);
                    let children = &self.plies[index].next;
                    if children.is_empty() {
                        let route = path.clone();
                        path.pop();
                        return Some(route);
                    }
                    branches.push(children.iter());
                } else {
                    branches.pop();
                    path.pop();
                }
            }
            None
        })
    }

    fn push(&mut self, ply: PgnPly) -> usize {
        let index = self.plies.len();
        match ply.previous {
            Some(previous) => self.plies[previous].next.push(index),
            None => self.roots.push(index),
        }
        self.plies.push(ply);
        index
    }
}

pub(super) fn parse_pgn(input: &str) -> Result<Vec<PgnGame<'_>>, ChessError> {
    parse_pgn_records(input)?
        .into_iter()
        .enumerate()
        .map(|(index, game)| {
            game.map_err(|error| invalid(format!("PGN game {}: {error}", index + 1)))
        })
        .collect()
}

/// Frame records before validating their moves, allowing one malformed game to
/// have a diagnostic without hiding subsequent independently framed games.
pub(super) fn parse_pgn_records(
    input: &str,
) -> Result<Vec<Result<PgnGame<'_>, ChessError>>, ChessError> {
    let mut games = Vec::new();
    let mut lexer = PgnTokens::new(input);
    let mut record = PgnRecord::default();
    loop {
        match lexer.next() {
            Ok(Some(token)) => {
                // A header after movetext is the next record, even if the
                // previous record omitted its result marker.
                if matches!(token, Token::Tag(..)) && record.movetext {
                    record.finish(&mut games);
                }
                match token {
                    Token::VariationStart => record.depth += 1,
                    Token::VariationEnd => record.depth = record.depth.saturating_sub(1),
                    _ => {}
                }
                let ended = matches!(token, Token::Result(_)) && record.depth == 0;
                record.movetext |= !matches!(token, Token::Tag(..) | Token::Comment(_));
                record.tokens.push(token);
                if ended {
                    record.finish(&mut games);
                }
            }
            Err(error) => {
                record.error.get_or_insert(error);
            }
            Ok(None) => break,
        }
    }
    record.finish(&mut games);
    if games.is_empty() {
        return Err(invalid("PGN contains no games"));
    }
    Ok(games)
}

#[derive(Default)]
struct PgnRecord<'a> {
    tokens: Vec<Token<'a>>,
    error: Option<ChessError>,
    movetext: bool,
    depth: usize,
}

impl<'a> PgnRecord<'a> {
    fn finish(&mut self, games: &mut Vec<Result<PgnGame<'a>, ChessError>>) {
        let record = std::mem::take(self);
        if record.error.is_none()
            && record
                .tokens
                .iter()
                .all(|token| matches!(token, Token::Comment(_)))
        {
            return;
        }
        games.push(match record.error {
            Some(error) => Err(error),
            None => parse_game(record.tokens.into_iter()),
        });
    }
}

fn parse_game<'a>(mut tokens: impl Iterator<Item = Token<'a>>) -> Result<PgnGame<'a>, ChessError> {
    let mut game = PgnGame {
        tags: BTreeMap::new(),
        result: "",
        roots: Vec::new(),
        mainline: Vec::new(),
        plies: Vec::new(),
    };
    let mut token = next_required(&mut tokens)?;
    let mut comments = Vec::new();
    loop {
        match token {
            Token::Tag(name, value) => {
                if game.tags.insert(name, value).is_some() {
                    return Err(invalid(format!("duplicate PGN tag {name}")));
                }
            }
            Token::Comment(comment) if !comment.is_empty() => comments.push(comment.to_owned()),
            Token::Comment(_) => {}
            _ => break,
        }
        token = next_required(&mut tokens)?;
    }
    if game
        .tags
        .get("Variant")
        .is_some_and(|tag| *tag != "Standard")
    {
        return Err(invalid("PGN requires orthodox chess rules"));
    }
    if game.tags.get("Result").is_some_and(|tag| !is_result(tag)) {
        return Err(invalid("invalid PGN Result tag"));
    }
    let fen = game.tags.get("FEN");
    let setup = game.tags.get("SetUp").copied();
    let board = match (setup, fen) {
        (None | Some("1"), Some(fen)) => parse_position(fen, true)?,
        (None | Some("0"), None) => Board::default(),
        _ => return Err(invalid("PGN SetUp and FEN tags are inconsistent")),
    };
    parse_movetext(
        &mut game,
        &mut tokens,
        token,
        Line::new(board, None, comments),
    )?;
    if game.plies.is_empty() {
        return Err(invalid("PGN game contains no moves"));
    }
    Ok(game)
}

struct Line {
    previous: Option<usize>,
    board: Board,
    comments: Vec<String>,
    numbered: bool,
    last: Option<usize>,
    move_number: usize,
}

impl Line {
    fn new(board: Board, previous: Option<usize>, comments: Vec<String>) -> Self {
        Self {
            move_number: usize::from(board.fullmove_number()),
            board,
            previous,
            comments,
            numbered: false,
            last: None,
        }
    }
}

fn parse_movetext<'a>(
    game: &mut PgnGame<'a>,
    tokens: &mut impl Iterator<Item = Token<'a>>,
    mut token: Token<'a>,
    mut line: Line,
) -> Result<(), ChessError> {
    let mut parents = Vec::new();
    loop {
        match token {
            Token::Tag(..) => return Err(invalid("misplaced PGN tag")),
            Token::MoveNumber { number, black } => {
                if line.numbered
                    || number != line.move_number
                    || black != (line.board.side_to_move() == Color::Black)
                {
                    return Err(invalid("PGN move number does not match the position"));
                }
                line.numbered = true;
            }
            Token::San(san) => {
                let move_number = line.move_number;
                let side = line.board.side_to_move();
                let illegal = || {
                    invalid(format!(
                        "invalid SAN {san:?} at move {move_number} ({side:?} to move)"
                    ))
                };
                let mv = parse_san_move(&line.board, san).map_err(|_| illegal())?;
                if display_san_move(&line.board, mv).to_string() != san {
                    return Err(illegal());
                }
                let index = game.push(PgnPly {
                    board: line.board.clone(),
                    san: san.to_owned(),
                    uci: display_uci_move(&line.board, mv).to_string(),
                    mv,
                    comments: std::mem::take(&mut line.comments),
                    previous: line.previous,
                    next: Vec::new(),
                });
                if parents.is_empty() {
                    game.mainline.push(index);
                }
                // The board's FEN counter saturates at u16::MAX. PGN move numbers
                // continue to count completed full moves independently.
                if side == Color::Black {
                    line.move_number += 1;
                }
                line.board.play_unchecked(mv);
                line.last = Some(index);
                line.previous = line.last;
                line.numbered = false;
            }
            Token::Comment(comment) if !comment.is_empty() => {
                match line.last.filter(|_| !line.numbered) {
                    Some(index) => game.plies[index].comments.push(comment.to_owned()),
                    None => line.comments.push(comment.to_owned()),
                }
            }
            Token::Comment(_) => {}
            Token::VariationStart => {
                let anchor = line
                    .last
                    .filter(|_| !line.numbered)
                    .ok_or_else(|| invalid("PGN variation requires a preceding move"))?;
                let ply = &game.plies[anchor];
                let mut variation = Line::new(ply.board.clone(), ply.previous, Vec::new());
                variation.move_number =
                    line.move_number - usize::from(ply.board.side_to_move() == Color::Black);
                parents.push(std::mem::replace(&mut line, variation));
            }
            Token::VariationEnd => {
                let parent = parents
                    .pop()
                    .ok_or_else(|| invalid("unexpected PGN variation closing delimiter"))?;
                if line.last.is_none() || line.numbered {
                    return Err(invalid("PGN variation requires a complete move"));
                }
                line = parent;
            }
            Token::Result(result) => {
                if !parents.is_empty()
                    || line.numbered
                    || game.tags.get("Result").is_some_and(|tag| *tag != result)
                {
                    return Err(invalid("PGN result does not match the game"));
                }
                game.result = result;
                return Ok(());
            }
        }
        token = next_required(tokens)?;
    }
}

fn next_required<'a>(
    tokens: &mut impl Iterator<Item = Token<'a>>,
) -> Result<Token<'a>, ChessError> {
    tokens
        .next()
        .ok_or_else(|| invalid("PGN game requires a result marker and closed variations"))
}

pub(super) fn invalid(message: impl Into<String>) -> ChessError {
    ChessError::InvalidContent(message.into())
}
