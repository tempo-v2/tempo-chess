//! Rank real board positions by shared pawn structure and piece placement.
use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::{ChessError, PositionKey};
use cozy_chess::{Board, Color, File, Piece};

/// Return nearby canonical learner positions from the caller's eligible catalog.
/// Same-position transpositions share their key and are excluded from the results.
pub fn structural_neighbors<'a>(
    fen: &str,
    candidates: impl IntoIterator<Item = &'a PositionKey>,
    limit: usize,
) -> Result<Vec<PositionKey>, ChessError> {
    let position = parse_position(fen, false)?;
    let own_key = board_key(&position);
    let baseline = Structure::read(&position);
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
    Ok(ranked
        .into_iter()
        .take(limit)
        .map(|(_, key)| key.clone())
        .collect())
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
