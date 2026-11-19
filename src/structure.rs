//! Rank real board positions by shared pawn structure and piece placement.
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::standard_uci::legal_move;
use crate::{ChessAnswer, ChessError, PositionKey};
use cozy_chess::util::display_san_move;
use cozy_chess::{Board, Color, File, Piece};
use serde::Serialize;
use std::collections::BTreeMap;

/// Return nearby canonical learner positions from the caller's eligible catalog.
/// Same-position transpositions share their key and are excluded from the results.
pub fn structural_neighbors<'a>(
    fen: &str,
    candidates: impl IntoIterator<Item = &'a PositionKey>,
    limit: usize,
) -> Result<Vec<PositionKey>, ChessError> {
    let position = parse_position(fen, false)?;
    Ok(rank_positions(&position, candidates)?
        .into_iter()
        .take(limit)
        .map(|(_, key)| key.clone())
        .collect())
}

/// A prescribed response from an active saved repertoire position.
pub struct SavedRepertoireResponse {
    pub repertoire_id: String,
    pub answer: ChessAnswer,
}

#[derive(Serialize)]
pub struct NearbyResponse {
    pub repertoire_id: String,
    pub uci: String,
    pub san: String,
    /// The same move and piece type can be played in the requested position.
    pub reusable: bool,
}

#[derive(Serialize)]
pub struct NearbyPosition {
    pub fen: String,
    pub distance: u32,
    pub responses: Vec<NearbyResponse>,
}

#[derive(Default, Serialize)]
pub struct NearbyRepertoirePositions {
    pub current: Vec<NearbyPosition>,
    pub other: Vec<NearbyPosition>,
}

/// Project the closest distinct same-side positions for the current repertoire
/// and other repertoires. Preserve their own SAN even if a move cannot be reused.
pub fn nearby_repertoire_positions(
    fen: &str,
    repertoire_id: &str,
    candidates: &BTreeMap<PositionKey, Vec<SavedRepertoireResponse>>,
    limit_per_group: usize,
) -> Result<NearbyRepertoirePositions, ChessError> {
    let position = parse_position(fen, false)?;
    let mut nearby = NearbyRepertoirePositions::default();
    for (distance, key) in rank_positions(&position, candidates.keys())? {
        let (current, other): (Vec<_>, Vec<_>) = candidates[key]
            .iter()
            .partition(|entry| entry.repertoire_id == repertoire_id);
        for (group, entries) in [(&mut nearby.current, current), (&mut nearby.other, other)] {
            if entries.is_empty() || group.len() == limit_per_group {
                continue;
            }
            let fen = format!("{} 0 1", key.0);
            let board = parse_position(&fen, false)?;
            let responses = entries
                .into_iter()
                .map(|entry| {
                    let uci = &entry.answer.uci;
                    let mv = legal_move(&board, uci)?;
                    let reusable = legal_move(&position, uci)
                        .is_ok_and(|here| board.piece_on(mv.from) == position.piece_on(here.from));
                    Ok(NearbyResponse {
                        repertoire_id: entry.repertoire_id.clone(),
                        uci: uci.clone(),
                        san: display_san_move(&board, mv).to_string(),
                        reusable,
                    })
                })
                .collect::<Result<_, ChessError>>()?;
            group.push(NearbyPosition {
                fen,
                distance,
                responses,
            });
        }
        if nearby.current.len() == limit_per_group && nearby.other.len() == limit_per_group {
            break;
        }
    }
    Ok(nearby)
}

fn rank_positions<'a>(
    position: &Board,
    candidates: impl IntoIterator<Item = &'a PositionKey>,
) -> Result<Vec<(u32, &'a PositionKey)>, ChessError> {
    let own_key = board_key(position);
    let baseline = Structure::read(position);
    let mut ranked = Vec::new();
    for key in candidates {
        if *key == own_key {
            continue;
        }
        let candidate = parse_position(&format!("{} 0 1", key.0), false)?;
        if position.side_to_move() == candidate.side_to_move() {
            ranked.push((baseline.distance(&Structure::read(&candidate)), key));
        }
    }
    ranked.sort_unstable();
    ranked.dedup();
    Ok(ranked)
}

struct Structure {
    placement: [u64; 12],
    pawn_files: [u32; 16],
    castling: [bool; 4],
}

impl Structure {
    fn read(board: &Board) -> Self {
        let mut structure = Self {
            placement: [0; 12],
            pawn_files: [0; 16],
            castling: [false; 4],
        };
        for (index, color) in [Color::White, Color::Black].into_iter().enumerate() {
            for (piece_index, piece) in [
                Piece::Pawn,
                Piece::Knight,
                Piece::Bishop,
                Piece::Rook,
                Piece::Queen,
                Piece::King,
            ]
            .into_iter()
            .enumerate()
            {
                structure.placement[index * 6 + piece_index] = board.colored_pieces(color, piece).0;
            }
            for (file_index, file) in File::ALL.into_iter().enumerate() {
                structure.pawn_files[index * 8 + file_index] =
                    (board.colored_pieces(color, Piece::Pawn) & file.bitboard()).len();
            }
            let rights = board.castle_rights(color);
            structure.castling[index * 2] = rights.short.is_some();
            structure.castling[index * 2 + 1] = rights.long.is_some();
        }
        structure
    }

    fn distance(&self, other: &Self) -> u32 {
        let piece_squares: u32 = self
            .placement
            .into_iter()
            .zip(other.placement)
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        let pawn_files: u32 = self
            .pawn_files
            .into_iter()
            .zip(other.pawn_files)
            .map(|(a, b)| a.abs_diff(b))
            .sum();
        let castling = self
            .castling
            .into_iter()
            .zip(other.castling)
            .filter(|(a, b)| a != b)
            .count() as u32;
        piece_squares + 2 * pawn_files + 2 * castling
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PositionCanonicalizer, START_FEN, StandardPositionCanonicalizer};

    #[test]
    fn nearby_examples_keep_source_notation_and_only_reuse_matching_legal_pieces() {
        let gap = "4k3/8/8/8/8/8/8/R3K3 w - - 0 1";
        let nearby = "4k3/8/8/8/8/8/7P/R3K3 w - - 0 1";
        let queen = "4k3/8/8/8/8/8/8/Q3K3 w - - 0 1";
        let canonicalizer = StandardPositionCanonicalizer;
        let key = |fen| canonicalizer.canonicalize(fen).unwrap();
        let saved = |id: &str, uci| SavedRepertoireResponse {
            repertoire_id: id.into(),
            answer: ChessAnswer::from(uci),
        };
        let candidates = BTreeMap::from([
            (key(gap), vec![saved("book", "a1a2")]),
            (
                key(nearby),
                vec![
                    saved("book", "a1a2"),
                    saved("two", "h2h3"),
                    saved("three", "a1a2"),
                ],
            ),
            (key(queen), vec![saved("other", "a1a2")]),
        ]);
        let result = nearby_repertoire_positions(gap, "book", &candidates, 2).unwrap();
        assert_eq!(result.current.len(), 1);
        assert_eq!(result.other.len(), 2);
        assert_eq!(result.current[0].responses[0].san, "Ra2");
        assert!(result.current[0].responses[0].reusable);
        assert_eq!(
            result
                .other
                .iter()
                .flat_map(|p| &p.responses)
                .filter(|mv| mv.reusable)
                .count(),
            1
        );
        let queen_move = result
            .other
            .iter()
            .flat_map(|p| &p.responses)
            .find(|response| response.repertoire_id == "other")
            .unwrap();
        assert_eq!(queen_move.san, "Qa2");
        assert!(!queen_move.reusable);
        assert!(
            !result
                .other
                .iter()
                .flat_map(|p| &p.responses)
                .find(|response| response.uci == "h2h3")
                .unwrap()
                .reusable
        );
    }

    #[test]
    fn ranking_ignores_counters_and_uses_legal_piece_and_pawn_placement() {
        let canonicalizer = StandardPositionCanonicalizer;
        let same = canonicalizer.canonicalize(START_FEN).unwrap();
        let knight = canonicalizer
            .canonicalize("rnbqkbnr/pppppppp/8/8/8/2N5/PPPPPPPP/R1BQKBNR w KQkq - 0 1")
            .unwrap();
        let pawn = canonicalizer
            .canonicalize("rnbqkbnr/pppppppp/8/8/2P5/8/PPP1PPPP/RNBQKBNR w KQkq - 0 1")
            .unwrap();
        let different_side = canonicalizer
            .canonicalize(&START_FEN.replace(" w ", " b "))
            .unwrap();
        let fen = START_FEN.replace("0 1", "30 100");
        assert_eq!(
            structural_neighbors(&fen, [&different_side, &pawn, &knight, &same], 2).unwrap(),
            vec![knight, pawn]
        );
    }
}
