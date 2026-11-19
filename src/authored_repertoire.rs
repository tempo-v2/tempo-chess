//! Legal authored positions, reply coverage and the route needed to extend a gap.
use crate::parsed_pgn::{invalid, parse_pgn};
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::{ChessError, LearnerColor, PlayedGame, PositionKey};
use cozy_chess::util::{display_san_move, display_uci_move, parse_uci_move};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct AuthoredRepertoire {
    pub positions: BTreeMap<PositionKey, AuthoredPosition>,
    pub routes: Vec<Vec<(PositionKey, String)>>,
}

#[derive(Debug)]
pub struct AuthoredPosition {
    pub side: LearnerColor,
    pub prefix: Vec<String>,
    pub replies: BTreeMap<String, PositionKey>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepartureKind {
    LearnerMove,
    OpponentGap,
    AuthoredEndpoint,
    OutsideContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Departure {
    pub ply: usize,
    pub kind: DepartureKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepertoireAdherence {
    pub repertoire_id: String,
    pub source_available: bool,
    pub first_departure: Option<Departure>,
    pub reentry_ply: Option<usize>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct OpponentGap {
    pub repertoire_id: String,
    pub position: PositionKey,
    pub actual_move: String,
}

#[derive(Serialize)]
pub struct GapDraft {
    pub fen: String,
    pub legal_moves: Vec<String>,
    pub pgn: Option<String>,
}

impl AuthoredRepertoire {
    pub fn parse(pgn: &str) -> Result<Self, ChessError> {
        let mut book = Self::default();
        for game in parse_pgn(pgn)? {
            for route in game.routes() {
                let mut prefix = Vec::new();
                let mut edges = Vec::new();
                for index in route {
                    let ply = &game.plies[index];
                    let position = board_key(&ply.board);
                    let mut after = ply.board.clone();
                    after.play(ply.mv);
                    let next = board_key(&after);
                    book.positions
                        .entry(position.clone())
                        .or_insert_with(|| AuthoredPosition {
                            side: LearnerColor::from_board(ply.board.side_to_move()),
                            prefix: prefix.clone(),
                            replies: BTreeMap::new(),
                        })
                        .replies
                        .insert(ply.uci.clone(), next.clone());
                    prefix.push(ply.uci.clone());
                    book.positions
                        .entry(next)
                        .or_insert_with(|| AuthoredPosition {
                            side: LearnerColor::from_board(after.side_to_move()),
                            prefix: prefix.clone(),
                            replies: BTreeMap::new(),
                        });
                    edges.push((position, ply.uci.clone()));
                }
                book.routes.push(edges);
            }
        }
        Ok(book)
    }

    pub fn compare(
        &self,
        repertoire_id: &str,
        game: &PlayedGame,
        color: LearnerColor,
    ) -> (RepertoireAdherence, BTreeSet<OpponentGap>) {
        let mut report = RepertoireAdherence {
            repertoire_id: repertoire_id.into(),
            source_available: true,
            first_departure: None,
            reentry_ply: None,
        };
        let mut gaps = BTreeSet::new();
        let mut outside = false;
        for (index, (position, side, played)) in game.moves.iter().enumerate() {
            let ply = index + 1;
            let kind = match self.positions.get(position) {
                Some(node) => {
                    if outside {
                        report.reentry_ply.get_or_insert(ply);
                        outside = false;
                    }
                    if node.replies.is_empty() {
                        DepartureKind::AuthoredEndpoint
                    } else if node.replies.contains_key(played) {
                        continue;
                    } else if *side == color.board_color() {
                        DepartureKind::LearnerMove
                    } else {
                        gaps.insert(OpponentGap {
                            repertoire_id: repertoire_id.into(),
                            position: position.clone(),
                            actual_move: played.clone(),
                        });
                        DepartureKind::OpponentGap
                    }
                }
                None => DepartureKind::OutsideContext,
            };
            report
                .first_departure
                .get_or_insert(Departure { ply, kind });
            outside = true;
        }
        (report, gaps)
    }

    /// Return an editable draft only for a legal move outside the currently authored replies.
    pub fn gap_draft(
        &self,
        pgn: &str,
        position: &PositionKey,
        opponent_move: &str,
        color: LearnerColor,
        response: Option<&str>,
    ) -> Result<GapDraft, ChessError> {
        let node = self
            .positions
            .get(position)
            .filter(|node| !node.replies.is_empty() && !node.replies.contains_key(opponent_move))
            .ok_or_else(|| invalid("This position no longer has the selected coverage gap"))?;
        let mut board = cozy_chess::Board::default();
        let mut path = Vec::new();
        for uci in &node.prefix {
            path.push(play_san(&mut board, uci)?);
        }
        if board_key(&board) != *position || board.side_to_move() == color.board_color() {
            return Err(invalid(
                "The authored route does not reach this opponent position",
            ));
        }
        path.push(play_san(&mut board, opponent_move)?);
        let mut legal_moves = Vec::new();
        board.generate_moves(|moves| {
            legal_moves.extend(
                moves
                    .into_iter()
                    .map(|mv| display_uci_move(&board, mv).to_string()),
            );
            false
        });
        legal_moves.sort_unstable();
        let fen = board.to_string();
        let pgn = response
            .map(|uci| {
                path.push(play_san(&mut board, uci)?);
                Ok(format!(
                    "{}\n\n[Event \"Response to opponent gap\"]\n[Result \"*\"]\n\n{} *\n",
                    pgn.trim_end(),
                    path.join(" ")
                ))
            })
            .transpose()?;
        Ok(GapDraft {
            fen,
            legal_moves,
            pgn,
        })
    }
}

fn play_san(board: &mut cozy_chess::Board, uci: &str) -> Result<String, ChessError> {
    let mv = parse_uci_move(board, uci).map_err(|_| invalid("Invalid UCI move"))?;
    if !board.is_legal(mv) {
        return Err(invalid("Illegal move in repertoire route"));
    }
    let san = display_san_move(board, mv).to_string();
    board.play(mv);
    Ok(san)
}

/// Legal replies in the exact position represented by a model cache key.
pub fn legal_model_moves(position: &PositionKey) -> Result<Vec<String>, ChessError> {
    let board = parse_position(&format!("{} 0 1", position.0), false)?;
    let mut moves = Vec::new();
    board.generate_moves(|set| {
        moves.extend(
            set.into_iter()
                .map(|mv| display_uci_move(&board, mv).to_string()),
        );
        false
    });
    moves.sort_unstable();
    Ok(moves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_played_games;

    #[test]
    fn variations_are_covered_and_first_gap_survives_canonical_reentry() {
        let pgn = "1. Nf3 Nf6 (1... d5 2. d4) 2. g3 g6 3. Bg2 *";
        let color = LearnerColor::White;
        let book = AuthoredRepertoire::parse(pgn).unwrap();
        let compare = |pgn| {
            let game = parse_played_games(pgn).unwrap().pop().unwrap().unwrap();
            book.compare("book", &game, color)
        };
        let (variation, gaps) = compare("1. Nf3 d5 2. d4 1-0");
        assert!(variation.first_departure.is_none());
        assert!(gaps.is_empty());
        let (endpoint, gaps) = compare("1. Nf3 d5 2. d4 e6 1-0");
        assert_eq!(
            endpoint.first_departure.unwrap().kind,
            DepartureKind::AuthoredEndpoint
        );
        assert!(gaps.is_empty());
        let (reentry, gaps) = compare("1. Nf3 g6 2. g3 Nf6 3. Bg2 1-0");
        assert_eq!(
            reentry.first_departure.as_ref().unwrap().kind,
            DepartureKind::OpponentGap
        );
        assert_eq!(reentry.first_departure.unwrap().ply, 2);
        assert_eq!(reentry.reentry_ply, Some(5));
        assert_eq!(gaps.len(), 1);
        let gap = gaps.first().unwrap();
        let draft = book
            .gap_draft(pgn, &gap.position, &gap.actual_move, color, Some("g2g3"))
            .unwrap();
        assert!(draft.legal_moves.contains(&"g2g3".into()));
        assert_eq!(draft.fen.split_whitespace().nth(1), Some("w"));
        let edited = AuthoredRepertoire::parse(&draft.pgn.unwrap()).unwrap();
        assert!(edited.positions[&gap.position].replies.contains_key("g7g6"));
        assert!(edited.positions[&gap.position].replies.contains_key("d7d5"));
    }
}
