//! Verified teaching evidence for a revealed tactic move.
use crate::hanging_capture::hanging_capture_target;
use crate::personal_tactics::{
    absolute_pins, collect_attack_patterns, pin_blocks_recapture, subject,
};
use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;
use crate::{BoardSubject, ChessAnswer, ChessError, Motif, PieceRole};
use cozy_chess::{Board, GameStatus, Move, Piece, Square};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

/// Pieces involved in the correction. `captured` identifies a piece before
/// the move, including the pawn square for en passant.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TacticExplanation {
    Pin {
        attacker: BoardSubject,
        front: BoardSubject,
        rear: BoardSubject,
    },
    PinCapture {
        attacker: BoardSubject,
        front: BoardSubject,
        rear: BoardSubject,
        captured: BoardSubject,
    },
    Skewer {
        attacker: BoardSubject,
        front: BoardSubject,
        rear: BoardSubject,
    },
    Fork {
        attacker: BoardSubject,
        targets: [BoardSubject; 2],
    },
    DiscoveredAttack {
        attacker: BoardSubject,
        target: BoardSubject,
    },
    DiscoveredCheck {
        attacker: BoardSubject,
        target: BoardSubject,
    },
    MatingLine,
    MateInOne {
        king: BoardSubject,
        checkers: Vec<BoardSubject>,
    },
    HangingPiece {
        captured: BoardSubject,
    },
    WinningExchange {
        captured: BoardSubject,
        defender: BoardSubject,
    },
}

impl TacticExplanation {
    fn order(&self) -> (u8, &str, &str, &str) {
        match self {
            Self::MatingLine | Self::MateInOne { .. } => (0, "", "", ""),
            Self::HangingPiece { .. } | Self::WinningExchange { .. } => (7, "", "", ""),
            Self::Pin {
                attacker,
                front,
                rear,
            }
            | Self::PinCapture {
                attacker,
                front,
                rear,
                ..
            } => {
                let rank = if rear.role == PieceRole::King { 1 } else { 2 };
                (rank, &attacker.square, &front.square, &rear.square)
            }
            Self::Skewer {
                attacker,
                front,
                rear,
            } => (3, &attacker.square, &front.square, &rear.square),
            Self::Fork { attacker, targets } => {
                (4, &attacker.square, &targets[0].square, &targets[1].square)
            }
            Self::DiscoveredCheck { attacker, target } => (5, &attacker.square, &target.square, ""),
            Self::DiscoveredAttack { attacker, target } => {
                (6, &attacker.square, &target.square, "")
            }
        }
    }
}

/// Describe a concrete effect of one legal move, without evaluating alternatives
/// or continuations. The caller must already have authorized revealing this move.
pub fn explain_tactic_move(
    fen: &str,
    correction: &ChessAnswer,
) -> Result<Option<TacticExplanation>, ChessError> {
    explain_with_payoff(fen, correction, None)
}

/// Explain the revealed decision using the issued, immutable continuation.
/// Only subjects already visible at this decision leave Chess.
pub fn explain_tactic_decision(
    decisions: &[crate::Decision],
) -> Result<Option<TacticExplanation>, ChessError> {
    let Some((decision, rest)) = decisions.split_first() else {
        return Ok(None);
    };
    let mut board = parse_position(&decision.fen, true)?;
    let initial = crate::tactic_payoff::material(&board, board.side_to_move());
    board.play_unchecked(legal_move(&board, &decision.expected.uci)?);
    let (payoff, reaches_mate) = payoff_subjects(
        board,
        initial,
        decision.autoplay.iter().chain(
            rest.iter()
                .flat_map(|d| std::iter::once(&d.expected).chain(&d.autoplay)),
        ),
    )?;
    if !rest.is_empty() && reaches_mate {
        return Ok(Some(TacticExplanation::MatingLine));
    }
    explain_with_payoff(&decision.fen, &decision.expected, payoff)
}

// Track both participants back to the correction board, including pieces that
// move again before the final capture. Castling uses king-to-rook internally.
fn payoff_subjects<'a>(
    mut board: Board,
    initial: i32,
    line: impl Iterator<Item = &'a ChessAnswer>,
) -> Result<(Option<(Square, Square)>, bool), ChessError> {
    let solver = !board.side_to_move();
    let mut origins = Square::ALL.map(Some);
    let mut last = None;
    for answer in line {
        let mv = legal_move(&board, &answer.uci)?;
        let turn = board.side_to_move();
        let castle = board.color_on(mv.to) == Some(turn);
        let capture = if board.color_on(mv.to) == Some(!turn) {
            Some(mv.to)
        } else if board.piece_on(mv.from) == Some(Piece::Pawn) && mv.from.file() != mv.to.file() {
            Some(Square::new(mv.to.file(), mv.from.rank()))
        } else {
            None
        };
        if turn == solver {
            last = origins[mv.from as usize].zip(capture.and_then(|sq| origins[sq as usize]));
        } else if capture.is_some() {
            last = None;
        }
        if let Some(square) = capture {
            origins[square as usize] = None;
        }
        let from = origins[mv.from as usize].take();
        if castle {
            let rook = origins[mv.to as usize].take();
            let kingside = mv.from.file() < mv.to.file();
            let rank = mv.from.rank();
            origins[Square::new(
                if kingside {
                    cozy_chess::File::G
                } else {
                    cozy_chess::File::C
                },
                rank,
            ) as usize] = from;
            origins[Square::new(
                if kingside {
                    cozy_chess::File::F
                } else {
                    cozy_chess::File::D
                },
                rank,
            ) as usize] = rook;
        } else {
            origins[mv.to as usize] = from;
        }
        board.play_unchecked(mv);
    }
    let gain_survives = last.is_some()
        && crate::tactic_payoff::material(&board, solver) > initial
        && (board.side_to_move() == solver
            || !board.generate_moves(|moves| {
                moves.into_iter().any(|mv| {
                    let mut reply = board.clone();
                    reply.play_unchecked(mv);
                    crate::tactic_payoff::material(&reply, solver) <= initial
                })
            }));
    Ok((
        last.filter(|_| gain_survives),
        board.side_to_move() != solver && board.status() == GameStatus::Won,
    ))
}

// Prove a material gain against the sole immediate recapture. Require the
// second attacker to be available in the original position as well as after
// the exchange, so an opened line alone cannot earn this explanation.
fn winning_exchange(board: &Board, after: &Board, mv: Move) -> Option<TacticExplanation> {
    let solver = board.side_to_move();
    if board.color_on(mv.to) != Some(!solver) || mv.promotion.is_some() {
        return None;
    }
    let mut recaptures = Vec::new();
    after.generate_moves(|moves| {
        recaptures.extend(moves.into_iter().filter(|reply| reply.to == mv.to));
        recaptures.len() > 1
    });
    let &[reply] = recaptures.as_slice() else {
        return None;
    };
    if reply.promotion.is_some() {
        return None;
    }
    let mut exchanged = after.clone();
    exchanged.play_unchecked(reply);
    let initial = crate::tactic_payoff::material(board, solver);
    let wins = exchanged.generate_moves(|moves| {
        moves.into_iter().any(|retake| {
            if retake.to != mv.to || retake.promotion.is_some() || !board.is_legal(retake) {
                return false;
            }
            let mut settled = exchanged.clone();
            settled.play_unchecked(retake);
            crate::tactic_payoff::material(&settled, solver) > initial
                && !settled.generate_moves(|moves| moves.into_iter().any(|reply| reply.to == mv.to))
        })
    });
    wins.then(|| TacticExplanation::WinningExchange {
        captured: subject(board, mv.to),
        defender: subject(board, reply.from),
    })
}

fn explain_with_payoff(
    fen: &str,
    correction: &ChessAnswer,
    payoff: Option<(Square, Square)>,
) -> Result<Option<TacticExplanation>, ChessError> {
    let board = parse_position(fen, true)?;
    let mv = legal_move(&board, &correction.uci)?;
    let mut after = board.clone();
    after.play_unchecked(mv);
    let defender = after.side_to_move();
    let king = after.king(defender);
    if after.status() == GameStatus::Won {
        return Ok(Some(TacticExplanation::MateInOne {
            king: subject(&after, king),
            checkers: after
                .checkers()
                .into_iter()
                .map(|sq| subject(&after, sq))
                .collect(),
        }));
    }

    let mut explanations = Vec::new();
    if board.color_on(mv.to) == Some(defender) {
        let existing_pins = absolute_pins(&board, defender);
        for (front, attacker) in absolute_pins(&after, defender) {
            if existing_pins.contains(&(front, attacker))
                && pin_blocks_recapture(&board, &after, mv.to, front)
            {
                explanations.push(TacticExplanation::PinCapture {
                    attacker: subject(&after, attacker),
                    front: subject(&after, front),
                    rear: subject(&after, king),
                    captured: subject(&board, mv.to),
                });
            }
        }
    }
    let mut patterns = Vec::new();
    let gained = collect_attack_patterns(
        &board,
        &after,
        mv,
        correction,
        &absolute_pins(&board, board.side_to_move()),
        &mut patterns,
    );
    for pattern in patterns {
        let [attacker] = pattern.attackers.as_slice() else {
            continue;
        };
        let attacker = attacker.clone();
        let explanation = match (pattern.motif, pattern.targets.as_slice()) {
            (Motif::Pin, [front, rear]) => TacticExplanation::Pin {
                attacker,
                front: front.clone(),
                rear: rear.clone(),
            },
            (Motif::Skewer, [front, rear]) => TacticExplanation::Skewer {
                attacker,
                front: front.clone(),
                rear: rear.clone(),
            },
            // King adjacency omits the safety of the king's proposed capture.
            (Motif::Fork, _) if attacker.role != PieceRole::King => {
                let mut targets = pattern.targets;
                targets.sort_by(|a, b| {
                    (Reverse(a.role as u8), &a.square).cmp(&(Reverse(b.role as u8), &b.square))
                });
                let from: Square = attacker.square.parse().expect("chess square");
                if let Some((captor, victim)) = payoff
                    && captor == from
                    && let Some(index) = targets.iter().position(|t| t.square == victim.to_string())
                {
                    let victim = targets.remove(index);
                    explanations.push(TacticExplanation::Fork {
                        attacker,
                        targets: [victim, targets.remove(0)],
                    });
                    continue;
                }
                let Some(index) = targets.iter().position(|target| {
                    gained.contains(&(from, target.square.parse().expect("chess square")))
                }) else {
                    continue;
                };
                let newly_attacked = targets.remove(index);
                TacticExplanation::Fork {
                    attacker,
                    targets: [newly_attacked, targets.remove(0)],
                }
            }
            (Motif::DiscoveredAttack, [target]) => TacticExplanation::DiscoveredAttack {
                attacker,
                target: target.clone(),
            },
            (Motif::DiscoveredCheck, [target]) => TacticExplanation::DiscoveredCheck {
                attacker,
                target: target.clone(),
            },
            _ => continue,
        };
        explanations.push(explanation);
    }
    let supported = |hint: &TacticExplanation| {
        payoff.is_some_and(|(captor, victim)| {
            let target = victim.to_string();
            match hint {
                TacticExplanation::Fork { attacker, targets } => {
                    attacker.square == captor.to_string()
                        && targets.iter().any(|p| p.square == target)
                }
                TacticExplanation::Pin { front, .. } => front.square == target,
                TacticExplanation::Skewer { attacker, rear, .. } => {
                    attacker.square == captor.to_string() && rear.square == target
                }
                TacticExplanation::DiscoveredAttack {
                    attacker,
                    target: prey,
                } => attacker.square == captor.to_string() && prey.square == target,
                _ => false,
            }
        })
    };
    // A material pin or skewer is useful here only if the issued continuation
    // realizes it. Absolute pins can independently constrain a legal reply.
    explanations.retain(|hint| match hint {
        TacticExplanation::Pin { rear, .. } if rear.role != PieceRole::King => supported(hint),
        TacticExplanation::Skewer { .. } => supported(hint),
        _ => true,
    });
    if let Some(captured) = hanging_capture_target(&board, mv) {
        explanations.push(TacticExplanation::HangingPiece { captured });
    }
    if let Some(exchange) = winning_exchange(&board, &after, mv) {
        explanations.push(exchange);
    }
    Ok(explanations.into_iter().min_by_key(|hint| {
        let priority = match hint {
            _ if supported(hint) => 0,
            TacticExplanation::PinCapture { .. } => 1,
            TacticExplanation::WinningExchange { .. } => 2,
            TacticExplanation::HangingPiece { .. } => 3,
            _ => 4,
        };
        (priority, hint.order().0)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explain(fen: &str, uci: &str) -> Option<TacticExplanation> {
        explain_tactic_move(fen, &uci.into()).unwrap()
    }

    #[test]
    fn explains_a_capture_without_recapture_and_marks_the_en_passant_pawn() {
        assert!(matches!(
            explain("7k/8/8/8/3n4/8/8/3R3K w - - 0 1", "d1d4"),
            Some(TacticExplanation::HangingPiece { captured })
                if captured.square == "d4" && captured.role == PieceRole::Knight
        ));
        assert_eq!(explain("7k/8/8/4p3/3n4/8/8/3R3K w - - 0 1", "d1d4"), None);
        assert!(matches!(
            explain("7k/8/8/3pP3/8/8/8/7K w - d6 0 1", "e5d6"),
            Some(TacticExplanation::HangingPiece { captured })
                if captured.square == "d5" && captured.role == PieceRole::Pawn
        ));
    }

    #[test]
    fn the_winning_exchange_explains_the_knight_capture_ahead_of_the_pin() {
        let fen = "r1bqk1r1/ppp1bpPp/2n5/8/2pN4/8/PPP2PPP/RNBQR1K1 b q - 0 1";
        let after_trade = "r1b1k1r1/ppp1bpPp/2n5/8/2pQ4/8/PPP2PPP/RNB1R1K1 b q - 0 2";
        let clue = explain_tactic_decision(&[
            crate::Decision {
                fen: fen.into(),
                expected: "d8d4".into(),
                autoplay: vec!["d1d4".into()],
            },
            crate::Decision {
                fen: after_trade.into(),
                expected: "c6d4".into(),
                autoplay: vec![],
            },
        ])
        .unwrap();
        assert!(matches!(
            clue,
            Some(TacticExplanation::WinningExchange { captured, defender })
                if captured.square == "d4" && captured.role == PieceRole::Knight
                    && defender.square == "d1" && defender.role == PieceRole::Queen
        ));
    }

    #[test]
    fn winning_exchange_requires_legal_backup_one_defender_and_a_settled_gain() {
        for fen in [
            // No backup.
            "2kq4/8/8/8/3N4/8/5P2/3Q2K1 b - - 0 1",
            // The c1 rook pins the knight to its king.
            "2kq4/8/2n5/8/3N4/8/5P2/2RQ2K1 b - - 0 1",
            // The white knight can also recapture.
            "2kq4/8/2n5/8/3N4/5N2/5P2/3Q2K1 b - - 0 1",
            // A queen for a rook and knight loses material.
            "2kq4/8/2n5/8/3N4/8/5P2/3R2K1 b - - 0 1",
            // The bishop can take the black knight after the queens come off.
            "2kq4/8/2n5/8/3N4/4Q3/5B2/6K1 b - - 0 1",
        ] {
            assert!(
                !matches!(
                    explain(fen, "d8d4"),
                    Some(TacticExplanation::WinningExchange { .. })
                ),
                "{fen}"
            );
        }
    }

    #[test]
    fn explains_the_pin_ray_and_distinguishes_an_exploited_pin() {
        let fen = "4k3/8/2n5/8/8/8/4B3/6K1 w - - 0 1";
        assert!(matches!(
            explain(fen, "e2b5"),
            Some(TacticExplanation::Pin { attacker, front, rear })
                if attacker.square == "b5" && attacker.role == PieceRole::Bishop
                    && front.square == "c6" && front.role == PieceRole::Knight
                    && rear.square == "e8" && rear.role == PieceRole::King
        ));
        let capture = "4k3/8/2n5/1B2p3/8/8/8/4R1K1 w - - 0 1";
        assert!(matches!(
            explain(capture, "e1e5"),
            Some(TacticExplanation::PinCapture { attacker, front, rear, captured })
                if attacker.square == "b5" && front.square == "c6"
                    && rear.square == "e8" && captured.square == "e5"
        ));
    }

    #[test]
    fn relative_rays_require_a_payoff_in_the_issued_line() {
        let explain_line = |fen: &str, reply: &str, finish: &str| {
            let mut board = parse_position(fen, true).unwrap();
            for uci in ["a1d1", reply] {
                board.play_unchecked(legal_move(&board, uci).unwrap());
            }
            explain_tactic_decision(&[
                crate::Decision {
                    fen: fen.into(),
                    expected: "a1d1".into(),
                    autoplay: vec![reply.into()],
                },
                crate::Decision {
                    fen: board.to_string(),
                    expected: finish.into(),
                    autoplay: vec![],
                },
            ])
            .unwrap()
        };
        let pin = "7k/8/3q4/8/3n4/8/8/R6K w - - 0 1";
        assert!(!matches!(
            explain(pin, "a1d1"),
            Some(TacticExplanation::Pin { .. })
        ));
        assert!(
            matches!(explain_line(pin,"d6e6","d1d4"),Some(TacticExplanation::Pin{front,..}) if front.square=="d4")
        );
        let skewer = "7k/8/3n4/8/3q4/8/8/R6K w - - 0 1";
        assert!(!matches!(
            explain(skewer, "a1d1"),
            Some(TacticExplanation::Skewer { .. })
        ));
        assert!(
            matches!(explain_line(skewer,"d4c4","d1d6"),Some(TacticExplanation::Skewer{rear,..}) if rear.square=="d6")
        );
        // On c5 the displaced queen protects d6. Rxd6 Qxd6 loses the exchange.
        assert!(!matches!(
            explain_line(skewer, "d4c5", "d1d6"),
            Some(TacticExplanation::Skewer { .. })
        ));
    }

    #[test]
    fn explains_a_fork_a_discovery_and_real_checkmate() {
        assert!(matches!(
            explain("4k3/3q4/6r1/8/8/5N2/8/K7 w - - 0 1", "f3e5"),
            Some(TacticExplanation::Fork { attacker, targets })
                if attacker.square == "e5" && targets[0].square == "d7" && targets[1].square == "g6"
        ));
        assert!(matches!(
            explain("7k/q7/8/8/8/8/N7/R6K w - - 0 1", "a2b4"),
            Some(TacticExplanation::DiscoveredAttack { attacker, target })
                if attacker.square == "a1" && target.square == "a7"
        ));
        assert!(matches!(
            explain("k7/8/8/8/8/8/N7/R6K w - - 0 1", "a2b4"),
            Some(TacticExplanation::DiscoveredCheck { attacker, target })
                if attacker.square == "a1" && target.square == "a8"
        ));
        assert!(matches!(
            explain("7k/5K2/6Q1/8/8/8/8/8 w - - 0 1", "g6g7"),
            Some(TacticExplanation::MateInOne { king, checkers })
                if king.square == "h8" && checkers.len() == 1 && checkers[0].square == "g7"
        ));
    }

    #[test]
    fn the_fork_includes_a_new_target_even_if_it_is_less_valuable() {
        let fen = "4k3/8/8/2p1r3/8/8/1B5K/q7 w - - 0 1";
        assert!(matches!(
            explain(fen, "b2d4"),
            Some(TacticExplanation::Fork { targets, .. })
                if targets[0].square == "c5" && targets[1].square == "a1"
        ));
    }

    #[test]
    fn a_king_cannot_fork_pieces_it_could_not_capture() {
        let fen = "8/8/8/4k3/3r1r2/8/5K2/8 w - - 0 1";
        assert_eq!(explain(fen, "f2e3"), None);
    }

    #[test]
    fn a_blocker_an_unrelated_move_or_a_pinned_attacker_does_not_make_a_clue() {
        assert_eq!(explain("4k3/3N4/2n5/8/8/8/4B3/6K1 w - - 0 1", "e2b5"), None);
        assert_eq!(explain("4k3/8/2n5/1B6/8/8/8/6K1 w - - 0 1", "g1h1"), None);
        assert_eq!(explain("4r2k/8/8/8/8/qb6/4R3/4K3 w - - 0 1", "e2e3"), None);
    }
}
