//! Position identity for orthodox chess, shared with repertoire import.

use crate::{ChessError, PositionCanonicalizer, PositionKey};
use cozy_chess::{Board, Color, File, Move, Piece, Rank, Square, get_king_moves, get_pawn_attacks};

/// Canonicalize standard chess FEN using the legal moves of the position.
/// Use this when assigning or comparing repertoire targets. The result has the
/// four position fields of FEN; import adds fixed counters for display.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardPositionCanonicalizer;

impl PositionCanonicalizer for StandardPositionCanonicalizer {
    fn canonicalize(&self, fen: &str) -> Result<PositionKey, ChessError> {
        let fields = fen.split_ascii_whitespace().take(7).collect::<Vec<_>>();
        let [pieces, turn, castling, ep, half, full] = fields.as_slice() else {
            return Err(ChessError::InvalidContent("expected six FEN fields".into()));
        };
        if !half.bytes().all(|b| b.is_ascii_digit())
            || !full.bytes().all(|b| b.is_ascii_digit())
            || full.bytes().all(|b| b == b'0')
        {
            return Err(ChessError::InvalidContent("invalid FEN counters".into()));
        }
        // Normalize numeric counters before parsing: the rules library otherwise
        // limits their range, although they cannot affect the position key.
        let board = Board::from_fen(&format!("{pieces} {turn} {castling} {ep} 0 1"), false)
            .map_err(|_| ChessError::InvalidContent("invalid standard chess FEN".into()))?;
        for side in [Color::White, Color::Black] {
            let rights = board.castle_rights(side);
            if (rights.short.is_some() || rights.long.is_some())
                && board.king(side).file() != File::E
            {
                return Err(ChessError::InvalidContent(
                    "castling rights require an orthodox king square".into(),
                ));
            }
        }
        if get_king_moves(board.king(Color::White)).has(board.king(Color::Black)) {
            return Err(ChessError::InvalidContent(
                "kings cannot be adjacent".into(),
            ));
        }
        let key = board_key(&board);
        // The reader accepts some nonstandard placement spellings. Its writer
        // emits exactly eight ranks with a single digit for each empty run.
        if key.0.split_ascii_whitespace().next() != Some(*pieces) {
            return Err(ChessError::InvalidContent(
                "invalid FEN piece placement".into(),
            ));
        }
        Ok(key)
    }
}

pub(crate) fn board_key(board: &Board) -> PositionKey {
    let fen = board.to_string();
    let mut fields = fen.split_ascii_whitespace();
    let pieces = fields.next().expect("board emits FEN");
    let turn = fields.next().expect("board emits side to move");
    let castling = fields.next().expect("board emits castling rights");
    let ep = fields.next().expect("board emits en passant");
    let ep = if let Some(file) = board.en_passant() {
        let turn = board.side_to_move();
        let to = Square::new(file, Rank::Sixth.relative_to(turn));
        if (get_pawn_attacks(to, !turn) & board.colored_pieces(turn, Piece::Pawn))
            .into_iter()
            .any(|from| {
                board.is_legal(Move {
                    from,
                    to,
                    promotion: None,
                })
            })
        {
            ep
        } else {
            "-"
        }
    } else {
        "-"
    };
    PositionKey(format!("{pieces} {turn} {castling} {ep}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::START_FEN;

    #[test]
    fn uses_board_side_and_castling_but_ignores_counters() {
        let canonicalizer = StandardPositionCanonicalizer;
        let initial = canonicalizer.canonicalize(START_FEN).unwrap();
        assert_eq!(initial.0, START_FEN.trim_end_matches(" 0 1"));
        assert_eq!(
            canonicalizer.canonicalize(&START_FEN.replace("0 1", "172 72983")),
            Ok(initial.clone())
        );
        for changed in [
            START_FEN.replace(" w ", " b "),
            START_FEN.replace("KQkq", "Qkq"),
        ] {
            assert_ne!(canonicalizer.canonicalize(&changed).unwrap(), initial);
        }
        for invalid in [
            "bogus",
            "8/8/8/8/8/8/8/8 w - - 0 1",
            "8/8/8/8/4k3/8/4K3 w - - 0 1",
            "8/8/8/8/8/4k3/8/04K3 w - - 0 1",
            "44/8/8/8/8/4k3/8/4K3 w - - 0 1",
            "8/8/8/8/8/8/4k3/4K3 w - - 0 1",
            &START_FEN.replace("0 1", "0 0"),
        ] {
            assert!(matches!(
                canonicalizer.canonicalize(invalid),
                Err(ChessError::InvalidContent(_))
            ));
        }
    }

    #[test]
    fn en_passant_is_part_of_identity_only_when_the_capture_is_legal() {
        for (position, effective) in [
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", true),
            ("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", false),
            ("4k3/8/8/3p4/8/8/8/4K3 w - d6 0 2", false),
            ("4k3/8/8/3pB3/8/8/8/4K3 w - d6 0 2", false),
            ("4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 2", true),
        ] {
            let key = StandardPositionCanonicalizer
                .canonicalize(position)
                .unwrap();
            let without = position.replace(" d6 ", " - ").replace(" d3 ", " - ");
            let without_key = StandardPositionCanonicalizer
                .canonicalize(&without)
                .unwrap();
            assert_eq!(key != without_key, effective, "{position}");
        }
    }
}
