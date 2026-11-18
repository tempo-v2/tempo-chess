//! Parse legal orthodox moves into a bounded tree shared by content compilers.

use crate::ChessError;
use crate::pgn_tokens::{PgnTokens, Token, is_result};
use crate::standard_position_canonicalizer::parse_position;
use cozy_chess::util::{display_san_move, display_uci_move, parse_san_move};
use cozy_chess::{Board, Color};
use std::collections::BTreeMap;

pub(super) const MAX_BYTES: usize = 256 * 1024;
pub(super) const MAX_GAMES: usize = 32;
pub(super) const MAX_PLIES: usize = 512;
const MAX_DEPTH: usize = 24;
const MAX_ROUTE_PLIES: usize = 32_768;

#[derive(Debug)]
pub(super) struct PgnPly {
    pub board: Board,
    pub san: String,
    pub uci: String,
    pub comments: Vec<String>,
    pub previous: Option<usize>,
    next: Vec<usize>,
}

#[derive(Debug)]
pub(super) struct PgnGame<'a> {
    pub tags: BTreeMap<&'a str, &'a str>,
    roots: Vec<usize>,
    pub plies: Vec<PgnPly>,
}

impl PgnGame<'_> {
    /// Enumerate finite root-to-leaf paths in PGN order (mainline first).
    pub fn routes(&self) -> Result<Vec<Vec<usize>>, ChessError> {
        let mut routes = Vec::new();
        let mut budget = MAX_ROUTE_PLIES;
        let mut path = Vec::new();
        self.visit_routes(&self.roots, &mut path, &mut budget, &mut routes)?;
        Ok(routes)
    }

    fn visit_routes(
        &self,
        next: &[usize],
        path: &mut Vec<usize>,
        budget: &mut usize,
        routes: &mut Vec<Vec<usize>>,
    ) -> Result<(), ChessError> {
        if next.is_empty() && !path.is_empty() {
            *budget = budget
                .checked_sub(path.len())
                .ok_or_else(|| invalid("PGN game expands to more than 32,768 route plies"))?;
            routes.push(path.clone());
        }
        for &index in next {
            path.push(index);
            self.visit_routes(&self.plies[index].next, path, budget, routes)?;
            path.pop();
        }
        Ok(())
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
    if input.len() > MAX_BYTES {
        return Err(invalid("PGN exceeds 256 KiB"));
    }
    let mut games = Vec::new();
    let mut tokens = PgnTokens::new(input);
    let mut comments = Vec::new();
    while let Some(first) = tokens.next()? {
        if let Token::Comment(comment) = first {
            if !comment.is_empty() {
                comments.push(comment.to_owned());
            }
            continue;
        }
        if games.len() == MAX_GAMES {
            return Err(invalid("PGN exceeds 32 games"));
        }
        let number = games.len() + 1;
        games.push(
            parse_game(first, &mut tokens, std::mem::take(&mut comments))
                .map_err(|error| invalid(format!("PGN game {number}: {error}")))?,
        );
    }
    if games.is_empty() {
        return Err(invalid("PGN contains no games"));
    }
    Ok(games)
}

fn parse_game<'a>(
    first: Token<'a>,
    tokens: &mut PgnTokens<'a>,
    mut comments: Vec<String>,
) -> Result<PgnGame<'a>, ChessError> {
    let mut game = PgnGame {
        tags: BTreeMap::new(),
        roots: Vec::new(),
        plies: Vec::new(),
    };
    let mut token = first;
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
        token = next_required(tokens)?;
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
    parse_line(&mut game, tokens, token, None, board, 0, comments)?;
    if game.plies.is_empty() {
        return Err(invalid("PGN game contains no moves"));
    }
    Ok(game)
}

fn parse_line<'a>(
    game: &mut PgnGame<'a>,
    tokens: &mut PgnTokens<'a>,
    mut token: Token<'a>,
    mut previous: Option<usize>,
    mut board: Board,
    depth: usize,
    mut comments: Vec<String>,
) -> Result<(), ChessError> {
    let mut numbered = false;
    let mut last = None;
    loop {
        match token {
            Token::Tag(..) => return Err(invalid("misplaced PGN tag")),
            Token::MoveNumber { number, black } => {
                if numbered
                    || number != board.fullmove_number()
                    || black != (board.side_to_move() == Color::Black)
                {
                    return Err(invalid("PGN move number does not match the position"));
                }
                numbered = true;
            }
            Token::San(san) => {
                if game.plies.len() == MAX_PLIES {
                    return Err(invalid("PGN game exceeds 512 plies including variations"));
                }
                let move_number = board.fullmove_number();
                let side = board.side_to_move();
                let illegal = || {
                    invalid(format!(
                        "invalid SAN {san:?} at move {move_number} ({side:?} to move)"
                    ))
                };
                let mv = parse_san_move(&board, san).map_err(|_| illegal())?;
                if display_san_move(&board, mv).to_string() != san {
                    return Err(illegal());
                }
                let index = game.push(PgnPly {
                    board: board.clone(),
                    san: san.to_owned(),
                    uci: display_uci_move(&board, mv).to_string(),
                    comments: std::mem::take(&mut comments),
                    previous,
                    next: Vec::new(),
                });
                // parse_san_move selects from the board's legal moves.
                board.play_unchecked(mv);
                last = Some(index);
                previous = last;
                numbered = false;
            }
            Token::Comment(comment) if !comment.is_empty() => match last.filter(|_| !numbered) {
                Some(index) => game.plies[index].comments.push(comment.to_owned()),
                None => comments.push(comment.to_owned()),
            },
            Token::Comment(_) => {}
            Token::VariationStart => {
                let anchor = last
                    .filter(|_| !numbered)
                    .ok_or_else(|| invalid("PGN variation requires a preceding move"))?;
                if depth == MAX_DEPTH {
                    return Err(invalid("PGN variation nesting exceeds 24 levels"));
                }
                let ply = &game.plies[anchor];
                let anchor_board = ply.board.clone();
                let anchor_previous = ply.previous;
                let first = next_required(tokens)?;
                parse_line(
                    game,
                    tokens,
                    first,
                    anchor_previous,
                    anchor_board,
                    depth + 1,
                    Vec::new(),
                )?;
            }
            Token::VariationEnd => {
                if depth == 0 {
                    return Err(invalid("unexpected PGN variation closing delimiter"));
                }
                if last.is_none() || numbered {
                    return Err(invalid("PGN variation requires a complete move"));
                }
                return Ok(());
            }
            Token::Result(result) => {
                if depth != 0
                    || numbered
                    || game.tags.get("Result").is_some_and(|tag| *tag != result)
                {
                    return Err(invalid("PGN result does not match the game"));
                }
                return Ok(());
            }
        }
        token = next_required(tokens)?;
    }
}

fn next_required<'a>(tokens: &mut PgnTokens<'a>) -> Result<Token<'a>, ChessError> {
    tokens
        .next()?
        .ok_or_else(|| invalid("PGN game requires a result marker and closed variations"))
}

pub(super) fn invalid(message: impl Into<String>) -> ChessError {
    ChessError::InvalidContent(message.into())
}
