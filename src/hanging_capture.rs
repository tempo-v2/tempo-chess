//! Concrete capture geometry shared by game detection and Study explanations.

use crate::{BoardSubject, LearnerColor, PieceRole};
use cozy_chess::{Board, Move, Piece, Square};

/// The opposing unit taken by a legal capture with no immediate legal recapture.
pub(super) fn hanging_capture_target(board: &Board, mv: Move) -> Option<BoardSubject> {
    let square = if board.color_on(mv.to) == Some(!board.side_to_move()) {
        mv.to
    } else if board.piece_on(mv.from) == Some(Piece::Pawn)
        && board.piece_on(mv.to).is_none()
        && mv.from.file() != mv.to.file()
    {
        Square::new(mv.to.file(), mv.from.rank())
    } else {
        return None;
    };
    if board.color_on(square) != Some(!board.side_to_move()) {
        return None;
    }
    let role = match board.piece_on(square)? {
        Piece::Pawn => PieceRole::Pawn,
        Piece::Knight => PieceRole::Knight,
        Piece::Bishop => PieceRole::Bishop,
        Piece::Rook => PieceRole::Rook,
        Piece::Queen => PieceRole::Queen,
        Piece::King => return None,
    };
    let mut after = board.clone();
    after.play_unchecked(mv);
    let recaptured =
        after.generate_moves(|replies| replies.into_iter().any(|reply| reply.to == mv.to));
    if recaptured {
        return None;
    }
    Some(BoardSubject {
        square: square.to_string(),
        role,
        color: LearnerColor::from_board(!board.side_to_move()),
    })
}
