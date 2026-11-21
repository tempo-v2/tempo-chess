//! Locate a realized tactical payoff on a bounded best-defense engine line.
use crate::{
    ChessAnswer, ChessError, standard_position_canonicalizer::parse_position,
    standard_uci::legal_move,
};
use cozy_chess::{Board, Color, GameStatus, Piece};

pub(super) fn material(board: &Board, solver: Color) -> i32 {
    [
        (Piece::Pawn, 1),
        (Piece::Knight, 3),
        (Piece::Bishop, 3),
        (Piece::Rook, 5),
        (Piece::Queen, 9),
    ]
    .into_iter()
    .map(|(piece, value)| {
        value
            * (board.colored_pieces(solver, piece).len() as i32
                - board.colored_pieces(!solver, piece).len() as i32)
    })
    .sum()
}

/// Only the selected root's engine variation is evidence. Return the shortest
/// settled gain relative to the starting material or actual checkmate.
/// Require a subsequent best-defense reply for nonterminal material claims.
pub fn tactical_payoff(
    fen: &str,
    root: &ChessAnswer,
    variation: &[String],
) -> Result<Option<Vec<ChessAnswer>>, ChessError> {
    if variation.first() != Some(&root.uci) {
        return Ok(None);
    }
    let mut board = parse_position(fen, true)?;
    let solver = board.side_to_move();
    let initial = material(&board, solver);
    let mut line = Vec::new();
    let mut endpoint = None;
    let mut settled = false;
    for uci in variation.iter().take(16) {
        let before = material(&board, solver);
        let mv = legal_move(&board, uci)?;
        board.play_unchecked(mv);
        line.push(ChessAnswer::from(uci.as_str()));
        let balance = material(&board, solver);
        let solver_moved = board.side_to_move() != solver;
        if board.status() == GameStatus::Won {
            if !solver_moved {
                return Ok(None);
            }
            if let Some(end) = endpoint.filter(|_| settled && balance > initial) {
                line.truncate(end);
            }
            return Ok(Some(line));
        }
        if board.status() == GameStatus::Drawn {
            return Ok(None);
        }
        if balance <= initial {
            // A later recapture or zwischenzug refutes the apparent gain.
            endpoint = None;
            settled = false;
        } else if solver_moved && balance > before && !settled {
            endpoint = Some(line.len());
        } else if !solver_moved && balance == before && board.checkers().is_empty() {
            settled |= endpoint.is_some();
        }
    }
    Ok(endpoint.filter(|_| settled).map(|end| {
        line.truncate(end);
        line
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Catalog, TacticExplanation, explain_tactic_decision, prepare_tactical_puzzle};
    const FORK: &str = "r2qk1nr/ppp3pp/2p1p3/2b5/4P3/8/PPPP1PPP/RNBQK2R w KQkq - 0 1";
    const DEFENDED: &str = "r3k2r/ppp2pp1/3p3p/4p2P/2B1P2q/3PB2P/PbP2P2/1R1Q1RK1 b kq - 0 1";

    fn pv(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn checking_fork_reaches_bishop_and_rescan_retains_the_target() {
        let root = "d1h5".into();
        let prepare = || prepare_tactical_puzzle(FORK, &root, &[]).unwrap();
        let fresh = prepare()
            .with_payoff(&pv("d1h5 g7g6 h5c5 g8f6"))
            .plan(&Catalog::default())
            .unwrap();
        let item = &fresh.content.catalog.items[&fresh.target];
        assert_eq!(item.definition.decisions.len(), 2);
        assert_eq!(
            item.definition.decisions[0].autoplay,
            [ChessAnswer::from("g7g6")]
        );
        assert_eq!(item.definition.decisions[1].expected.uci, "h5c5");
        assert!(
            matches!(explain_tactic_decision(&item.definition.decisions).unwrap(),Some(TacticExplanation::Fork{attacker,targets}) if attacker.square=="h5" && targets[0].square=="c5" && targets[1].square=="e8")
        );
        assert_eq!(
            prepare()
                .plan(&fresh.content.catalog)
                .unwrap()
                .content
                .catalog,
            fresh.content.catalog
        );
    }

    #[test]
    fn incomplete_evidence_and_imported_aliases_preserve_the_original_line() {
        let prepared = || prepare_tactical_puzzle(FORK, &"d1h5".into(), &[]).unwrap();
        let baseline = prepared().plan(&Catalog::default()).unwrap();
        let invalid = prepared()
            .with_payoff(&pv("d1h5 a7a3"))
            .plan(&Catalog::default())
            .unwrap();
        assert_eq!(invalid, baseline);
        let mut authored = baseline.content.catalog.items[&baseline.target].clone();
        authored.provenance.push("tactic:70676e:31".into());
        assert!(
            crate::extend_generated_tactic(&authored, &pv("d1h5 g7g6 h5c5 g8f6"))
                .unwrap()
                .is_none()
        );
        authored.target = tempo_types::TargetId::from("tactic:70676e:31");
        let authored_catalog = Catalog {
            items: [(authored.target.clone(), authored.clone())].into(),
            lines: vec![],
        };
        let repeat = prepared()
            .with_payoff(&pv("d1h5 g7g6 h5c5 g8f6"))
            .plan(&authored_catalog)
            .unwrap();
        assert_eq!(repeat.target, authored.target);
        assert_eq!(
            repeat.content.catalog.items[&repeat.target].definition,
            authored.definition
        );
    }

    #[test]
    fn takes_free_h_pawn_without_claiming_a_skewer() {
        let plan = prepare_tactical_puzzle(DEFENDED, &"h4h3".into(), &[])
            .unwrap()
            .with_payoff(&pv("h4h3 d1e2"))
            .plan(&Catalog::default())
            .unwrap();
        let decisions = &plan.content.catalog.items[&plan.target]
            .definition
            .decisions;
        assert_eq!(decisions.len(), 1);
        assert!(
            matches!(explain_tactic_decision(decisions).unwrap(),Some(TacticExplanation::HangingPiece{captured}) if captured.square=="h3")
        );
    }

    #[test]
    fn a_mating_sequence_explains_the_setup_without_disclosing_its_finish() {
        let fen = "8/5Q2/k2K4/8/8/8/8/8 w - - 0 1";
        let plan = prepare_tactical_puzzle(fen, &"d6c5".into(), &[])
            .unwrap()
            .with_payoff(&pv("d6c5 a6a5 f7a2"))
            .plan(&Catalog::default())
            .unwrap();
        let decisions = &plan.content.catalog.items[&plan.target]
            .definition
            .decisions;
        assert!(matches!(
            explain_tactic_decision(decisions).unwrap(),
            Some(TacticExplanation::MatingLine)
        ));
        assert!(matches!(
            explain_tactic_decision(&decisions[1..]).unwrap(),
            Some(TacticExplanation::MateInOne { .. })
        ));
        use tempo_study::Grader;
        let definition = &plan.content.catalog.items[&plan.target].definition;
        let alternate = crate::LegalSequenceGrader
            .answer(definition, &crate::ChessState::at(1), &"f7a7".into())
            .unwrap();
        assert!(alternate.complete && alternate.feedback.correct);
        assert_eq!(alternate.state.played_mate.unwrap().uci, "f7a7");
    }

    #[test]
    fn settled_capture_ends_before_later_mate() {
        let fen = "8/8/8/4r3/5Q2/K7/8/k7 w - - 0 1";
        assert_eq!(
            tactical_payoff(fen, &"f4e5".into(), &pv("f4e5 a1b1 e5b2"))
                .unwrap()
                .unwrap(),
            [ChessAnswer::from("f4e5")]
        );
    }

    #[test]
    fn exchanges_must_gain_from_the_starting_balance() {
        // White gives a rook for a pawn, then wins the recapturing queen.
        let fen = "3q3k/3p4/8/8/6B1/8/8/3R3K w - - 0 1";
        let gain = pv("d1d7 d8d7 g4d7 h8g7");
        assert_eq!(
            tactical_payoff(fen, &"d1d7".into(), &gain)
                .unwrap()
                .unwrap()
                .len(),
            3
        );
        let equal = "3r3k/3r4/8/8/8/8/8/3R3K w - - 0 1";
        assert_eq!(
            tactical_payoff(equal, &"d1d7".into(), &pv("d1d7 d8d7")).unwrap(),
            None
        );
        let mate = "7k/8/5KQ1/8/8/8/8/8 w - - 0 1";
        assert_eq!(
            tactical_payoff(mate, &"g6g7".into(), &pv("g6g7"))
                .unwrap()
                .unwrap()
                .len(),
            1
        );
    }
}
