//! Verified teaching evidence for a revealed tactic move.
use crate::personal_tactics::{
    absolute_pins, collect_attack_patterns, pin_blocks_recapture, subject,
};
use crate::standard_position_canonicalizer::parse_position;
use crate::standard_uci::legal_move;
use crate::{BoardSubject, ChessAnswer, ChessError, Motif, PieceRole};
use cozy_chess::{GameStatus, Square};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

/// Pieces involved in the correction after the move. In a pin capture,
/// `captured` identifies the piece that was on the destination before the move.
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
    MateInOne {
        king: BoardSubject,
        checkers: Vec<BoardSubject>,
    },
}

impl TacticExplanation {
    fn order(&self) -> (u8, &str, &str, &str) {
        match self {
            Self::MateInOne { .. } => (0, "", "", ""),
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
    Ok(explanations
        .into_iter()
        .min_by(|a, b| a.order().cmp(&b.order())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explain(fen: &str, uci: &str) -> Option<TacticExplanation> {
        explain_tactic_move(fen, &uci.into()).unwrap()
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
    fn distinguishes_the_front_and_rear_pieces_on_the_same_ray() {
        assert!(matches!(
            explain("7k/8/3b4/8/3n4/8/8/R6K w - - 0 1", "a1d1"),
            Some(TacticExplanation::Pin { attacker, front, rear })
                if attacker.square == "d1" && front.square == "d4" && rear.square == "d6"
        ));
        assert!(matches!(
            explain("7k/8/3n4/8/3b4/8/8/R6K w - - 0 1", "a1d1"),
            Some(TacticExplanation::Skewer { attacker, front, rear })
                if attacker.square == "d1" && front.square == "d4" && rear.square == "d6"
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
