//! Verify the connected motifs demonstrated by a candidate game continuation.
use crate::{
    BoardSubject, ChessAnswer, ChessDefinition, ChessError, Decision, Motif, MotifCandidate,
    hanging_capture::hanging_capture_target,
    personal_tactics::{
        absolute_pins, collect_attack_patterns, mate_moves, pin_blocks_recapture, subject,
    },
    standard_position_canonicalizer::parse_position,
    standard_uci::legal_move,
};
use cozy_chess::{Board, Color, GameStatus, Move, Piece, Square};

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

/// A verified winning segment. The move indexes address the legal learner
/// line, including its intervening opponent replies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TacticalPayoff {
    pub line: Vec<ChessAnswer>,
    pub motifs: Vec<PayoffMotif>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PayoffMotif {
    pub setup: usize,
    pub finish: usize,
    pub motif: Motif,
    pub attackers: Vec<BoardSubject>,
    pub targets: Vec<BoardSubject>,
    pub material_gain: i32,
}

struct Step {
    before: Board,
    after: Board,
    mv: Move,
    answer: ChessAnswer,
}

fn board_moves(board: &Board) -> Vec<Move> {
    let mut legal = Vec::new();
    board.generate_moves(|moves| {
        legal.extend(moves);
        false
    });
    legal
}

fn play(board: &Board, mv: Move) -> Board {
    let mut after = board.clone();
    after.play_unchecked(mv);
    after
}

fn captures(board: &Board, mv: Move) -> bool {
    board.color_on(mv.to) == Some(!board.side_to_move())
        || (board.piece_on(mv.from) == Some(Piece::Pawn) && mv.from.file() != mv.to.file())
}

fn exchange_holds(board: &Board, square: Square, solver: Color, baseline: i32) -> bool {
    // Either side can walk away from a local exchange. Threats elsewhere
    // and forced checks are examined separately by the endpoint check.
    let stop = material(board, solver) > baseline;
    if board.status() != GameStatus::Ongoing {
        return board.status() == GameStatus::Won && board.side_to_move() != solver;
    }
    let in_check = !board.checkers().is_empty();
    let retakes = || {
        board_moves(board)
            .into_iter()
            .filter(|&mv| mv.to == square && captures(board, mv))
            .map(|mv| exchange_holds(&play(board, mv), square, solver, baseline))
    };
    if board.side_to_move() == solver {
        (!in_check && stop)
            || retakes().any(|holds| holds)
            || (in_check
                && stop
                && board_moves(board).into_iter().any(|evasion| {
                    let evaded = play(board, evasion);
                    material(&evaded, solver) > baseline
                        && evaded.status() != GameStatus::Drawn
                        && !board_moves(&evaded).into_iter().any(|reply| {
                            let next = play(&evaded, reply);
                            next.status() == GameStatus::Won
                                || captures(&evaded, reply) && material(&next, solver) <= baseline
                        })
                }))
    } else {
        stop && retakes().all(|holds| holds)
    }
}

fn holds_after_response(board: &Board, reply: Move, solver: Color, baseline: i32) -> bool {
    let after = play(board, reply);
    if after.status() != GameStatus::Ongoing {
        return false;
    }
    if after.checkers().is_empty() {
        exchange_holds(&after, reply.to, solver, baseline)
    } else {
        // A forcing check can expose a new capture. Verify the evasion
        // against the next captures, without following positional play.
        board_moves(&after).into_iter().any(|evasion| {
            let evaded = play(&after, evasion);
            material(&evaded, solver) > baseline
                && evaded.status() != GameStatus::Drawn
                && board_moves(&evaded).into_iter().all(|response| {
                    let next = play(&evaded, response);
                    next.status() != GameStatus::Won
                        && (!captures(&evaded, response)
                            || exchange_holds(&next, response.to, solver, baseline))
                })
        })
    }
}

fn holds(board: &Board, solver: Color, baseline: i32) -> bool {
    material(board, solver) > baseline
        && board.status() == GameStatus::Ongoing
        && board_moves(board).into_iter().all(|reply| {
            // A quiet move cannot take the demonstrated gain. Check legal
            // captures, checks and terminal replies in every defense branch.
            let after = play(board, reply);
            if !captures(board, reply)
                && after.checkers().is_empty()
                && after.status() == GameStatus::Ongoing
            {
                return true;
            }
            holds_after_response(board, reply, solver, baseline)
        })
}

/// Whether a verified one-move hanging-material lesson has another capture
/// that independently retains at least the lesson's material gain.
/// Call for game-owned lessons at qualification or when rechecking a verdict.
pub fn ambiguous_hanging_material(
    definition: &ChessDefinition,
    motifs: &[PayoffMotif],
) -> Result<bool, ChessError> {
    let [
        Decision {
            fen,
            expected,
            autoplay,
        },
    ] = definition.decisions.as_slice()
    else {
        return Ok(false);
    };
    if !autoplay.is_empty() {
        return Ok(false);
    }
    // Older definitions can predate the stored motif. Prove the same
    // standalone move before making an admission decision for them.
    let proof;
    let motifs = if motifs.is_empty() {
        let Some(found) = recognizable_payoff(fen, expected, std::slice::from_ref(&expected.uci))?
        else {
            return Ok(false);
        };
        proof = found;
        proof.motifs.as_slice()
    } else {
        motifs
    };
    let [motif] = motifs else {
        return Ok(false);
    };
    if motif.motif != Motif::HangingMaterial
        || motif.setup != 0
        || motif.finish != 0
        || motif.material_gain <= 0
    {
        return Ok(false);
    }
    let board = parse_position(fen, true)?;
    let expected = legal_move(&board, &expected.uci)?;
    let solver = board.side_to_move();
    let after_expected = play(&board, expected);
    // Find the strongest material floor the original capture can keep.
    // The payoff check is strict, so a floor of n proves retention of n + 1.
    let Some(floor) = (material(&board, solver)..material(&after_expected, solver))
        .rev()
        .find(|&floor| holds(&after_expected, solver, floor))
    else {
        return Ok(false);
    };
    Ok(board_moves(&board)
        .into_iter()
        .filter(|&mv| mv != expected && captures(&board, mv))
        .any(|mv| {
            let after = play(&board, mv);
            // A checkmating capture has no defensive reply.
            (after.status() == GameStatus::Won && material(&after, solver) > floor)
                || holds(&after, solver, floor)
        }))
}

fn patterns(step: &Step) -> Vec<MotifCandidate> {
    let mut patterns = Vec::new();
    collect_attack_patterns(
        &step.before,
        &step.after,
        step.mv,
        &step.answer,
        &absolute_pins(&step.before, step.before.side_to_move()),
        &mut patterns,
    );
    patterns
}

fn uses_threat(pattern: &MotifCandidate, board: &Board, capture: Move) -> bool {
    captures(board, capture)
        && pattern
            .attackers
            .iter()
            .any(|attacker| attacker.square.parse::<Square>().ok() == Some(capture.from))
        && pattern
            .targets
            .iter()
            .skip(usize::from(pattern.motif == Motif::Skewer))
            .any(|target| target.square.parse::<Square>().ok() == Some(capture.to))
}

fn proved_threat(step: &Step, finish: &Step, baseline: i32) -> Option<MotifCandidate> {
    let solver = step.before.side_to_move();
    patterns(step).into_iter().find(|pattern| {
        uses_threat(pattern, &finish.before, finish.mv)
            && holds(&finish.after, solver, baseline)
            && forces_threat(&step.after, pattern, solver, baseline)
    })
}

fn forces_threat(after: &Board, pattern: &MotifCandidate, solver: Color, baseline: i32) -> bool {
    board_moves(after).into_iter().all(|defense| {
        let reply = play(after, defense);
        board_moves(&reply).into_iter().any(|capture| {
            let defended_target = captures(&reply, capture)
                && capture.to == defense.to
                && pattern
                    .targets
                    .iter()
                    .any(|target| target.square.parse::<Square>().ok() == Some(defense.from));
            (uses_threat(pattern, &reply, capture) || defended_target)
                && holds(&play(&reply, capture), solver, baseline)
        })
    })
}

fn forces_mate_next(after: &Board) -> bool {
    after.status() == GameStatus::Ongoing
        && board_moves(after).into_iter().all(|defense| {
            let reply = play(after, defense);
            board_moves(&reply)
                .into_iter()
                .any(|mate| play(&reply, mate).status() == GameStatus::Won)
        })
}

fn connected(previous: &Step, reply: &Step, step: &Step, motif: &MotifCandidate) -> bool {
    let forcing = !previous.after.checkers().is_empty();
    if forcing {
        let Ok(earlier_move) = legal_move(&previous.before, &step.answer.uci) else {
            return true;
        };
        let earlier = play(&previous.before, earlier_move);
        if !step.after.checkers().is_empty() && earlier.checkers().is_empty() {
            return true;
        }
        if step.after.status() == GameStatus::Won && earlier.status() != GameStatus::Won {
            return true;
        }
        if captures(&step.before, step.mv) {
            return !captures(&previous.before, earlier_move)
                || previous.before.piece_on(earlier_move.to) != step.before.piece_on(step.mv.to)
                || !holds(
                    &earlier,
                    step.before.side_to_move(),
                    material(&previous.before, step.before.side_to_move()),
                );
        }
        if motif.motif == Motif::MateInTwo {
            return !forces_mate_next(&earlier);
        }
        let prior_setup = Step {
            before: previous.before.clone(),
            after: earlier,
            mv: earlier_move,
            answer: step.answer.clone(),
        };
        let solver = prior_setup.before.side_to_move();
        let baseline = material(&prior_setup.before, solver);
        // A check links the next setup only when it created or enabled the
        // specific winning motif. A second tactic already available in the
        // same form is a separate lesson.
        return !patterns(&prior_setup).into_iter().any(|prior| {
            prior.motif == motif.motif
                && prior.attackers == motif.attackers
                && prior.targets == motif.targets
                && forces_threat(&prior_setup.after, &prior, solver, baseline)
        });
    }
    // A quiet finish may chain when the defender answers an attack created
    // by that finish and in doing so uncovers the next winning capture.
    let mut new_patterns = Vec::new();
    let threats = collect_attack_patterns(
        &previous.before,
        &previous.after,
        previous.mv,
        &previous.answer,
        &absolute_pins(&previous.before, previous.before.side_to_move()),
        &mut new_patterns,
    );
    let responds_to_tactic = threats.iter().any(|&(_, target)| target == reply.mv.from)
        || (captures(&reply.before, reply.mv) && reply.mv.to == previous.mv.to);
    responds_to_tactic
        && captures(&step.before, step.mv)
        && previous.after.color_on(step.mv.to) == step.before.color_on(step.mv.to)
        && !(cozy_chess::get_between_rays(step.mv.from, step.mv.to)
            & previous.after.occupied()
            & !step.before.occupied())
        .is_empty()
}

fn capture_motif(step: &Step) -> MotifCandidate {
    let before = &step.before;
    let pinned = absolute_pins(before, !before.side_to_move());
    let pin = pinned
        .into_iter()
        .find(|(pinned, _)| pin_blocks_recapture(before, &step.after, step.mv.to, *pinned));
    let target = step.mv.to;
    let victim = if before.piece_on(target).is_none() {
        Square::new(target.file(), step.mv.from.rank())
    } else {
        target
    };
    let mut attackers = vec![subject(before, step.mv.from)];
    let mut targets = vec![subject(before, victim)];
    if let Some((pinned, pinner)) = pin {
        attackers.push(subject(before, pinner));
        targets.push(subject(before, pinned));
    }
    MotifCandidate {
        motif: if pin.is_some() {
            Motif::Pin
        } else {
            Motif::HangingMaterial
        },
        answer: step.answer.clone(),
        attackers,
        targets,
        continuation: vec![],
    }
}

/// The engine supplies a candidate defense and an optional tail. Chess accepts
/// only a prefix with a demonstrated tactical payoff. A later engine preference
/// cannot erase an already verified finish or add another learner decision.
pub fn recognizable_payoff(
    fen: &str,
    root: &ChessAnswer,
    variation: &[String],
) -> Result<Option<TacticalPayoff>, ChessError> {
    if variation.first() != Some(&root.uci) {
        return Ok(None);
    }
    let mut board = parse_position(fen, true)?;
    let solver = board.side_to_move();
    let mut steps = Vec::new();
    for uci in variation {
        let mv = match legal_move(&board, uci) {
            Ok(mv) => mv,
            Err(error) if steps.is_empty() => return Err(error),
            Err(_) => break,
        };
        let after = play(&board, mv);
        steps.push(Step {
            before: board,
            after: after.clone(),
            mv,
            answer: ChessAnswer::from(uci.as_str()),
        });
        board = after;
    }
    let mut motifs = Vec::new();
    let mut index = 0;
    while let Some(step) = steps.get(index) {
        let previous = index.checked_sub(2).map(|i| &steps[i]);
        let baseline = material(&step.before, solver);
        let next = steps.get(index + 2);
        let mut finish = index;
        let motif = if step.after.status() == GameStatus::Won {
            MotifCandidate {
                motif: Motif::MateInOne,
                answer: step.answer.clone(),
                attackers: step
                    .after
                    .checkers()
                    .into_iter()
                    .map(|sq| subject(&step.after, sq))
                    .collect(),
                targets: vec![subject(&step.after, step.after.king(!solver))],
                continuation: vec![],
            }
        } else if captures(&step.before, step.mv)
            && holds(&step.after, solver, baseline)
            && hanging_capture_target(&step.before, step.mv).is_some()
        {
            capture_motif(step)
        } else if let Some(next) = next
            && next.after.status() == GameStatus::Won
            && forces_mate_next(&step.after)
        {
            finish = index + 2;
            MotifCandidate {
                motif: Motif::MateInTwo,
                answer: step.answer.clone(),
                attackers: vec![subject(&step.before, step.mv.from)],
                targets: vec![subject(&step.after, step.after.king(!solver))],
                continuation: vec![],
            }
        } else if let Some(next) = next
            && let Some(threat) = proved_threat(step, next, baseline)
        {
            finish = index + 2;
            threat
        } else if let (true, Some(reply), Some(next)) =
            (captures(&step.before, step.mv), steps.get(index + 1), next)
            && reply.mv.to == step.mv.to
            && captures(&reply.before, reply.mv)
            && next.mv.to == step.mv.to
            && captures(&next.before, next.mv)
            && holds(&next.after, solver, baseline)
            && board_moves(&step.after).into_iter().all(|defense| {
                if defense.to == step.mv.to && captures(&step.after, defense) {
                    let reply = play(&step.after, defense);
                    board_moves(&reply).into_iter().any(|take| {
                        take.to == step.mv.to
                            && captures(&reply, take)
                            && holds(&play(&reply, take), solver, baseline)
                    })
                } else {
                    holds_after_response(&step.after, defense, solver, baseline)
                }
            })
        {
            finish = index + 2;
            capture_motif(step)
        } else if let Some(next) = next
            && !step.after.checkers().is_empty()
            && let Some(discovered) = patterns(step)
                .into_iter()
                .find(|pattern| pattern.motif == Motif::DiscoveredCheck)
            && captures(&next.before, next.mv)
            && holds(&next.after, solver, baseline)
            && !board_moves(&step.before).into_iter().any(|earlier| {
                earlier.to == next.mv.to
                    && captures(&step.before, earlier)
                    && holds(&play(&step.before, earlier), solver, baseline)
            })
            && board_moves(&step.after).into_iter().all(|defense| {
                let reply = play(&step.after, defense);
                board_moves(&reply).into_iter().any(|take| {
                    take.to == next.mv.to
                        && captures(&reply, take)
                        && holds(&play(&reply, take), solver, baseline)
                })
            })
        {
            finish = index + 2;
            discovered
        } else {
            break;
        };
        if let Some(previous) = previous
            && !connected(previous, &steps[index - 1], step, &motif)
        {
            break;
        }
        let end = &steps[finish].after;
        // A saved alternate mate replays from the final decision's board.
        // Keep the last complete material payoff if a mating extension
        // would allow an earlier finish the saved attempt cannot address.
        if end.status() == GameStatus::Won
            && steps[..finish]
                .iter()
                .step_by(2)
                .any(|earlier| !mate_moves(&earlier.before).is_empty())
        {
            break;
        }
        motifs.push(PayoffMotif {
            setup: index,
            finish,
            motif: motif.motif,
            attackers: motif.attackers,
            targets: motif.targets,
            material_gain: material(end, solver) - baseline,
        });
        if end.status() == GameStatus::Won {
            break;
        }
        index = finish + 2;
    }
    let Some(last) = motifs.last() else {
        return Ok(None);
    };
    Ok(Some(TacticalPayoff {
        line: steps[..=last.finish]
            .iter()
            .map(|step| step.answer.clone())
            .collect(),
        motifs,
    }))
}

pub fn tactical_payoff(
    fen: &str,
    root: &ChessAnswer,
    variation: &[String],
) -> Result<Option<Vec<ChessAnswer>>, ChessError> {
    Ok(recognizable_payoff(fen, root, variation)?.map(|verified| verified.line))
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
    fn a_game_capture_drill_requires_one_proven_capture() {
        use crate::{GameTacticQualification, requalify_game_tactic};

        let ambiguous = "2k3rr/ppp5/1n2P2p/4Nn2/3P2pP/2N5/P4PP1/R4RK1 b - - 0 1";
        let single = ambiguous.replace("3P2pP", "3P2p1");
        let defended_alternative = ambiguous.replace("R4RK1", "5RKR");
        let expected = "f5d4".into();
        for (fen, withheld) in [
            (ambiguous, true),
            (single.as_str(), false),
            (defended_alternative.as_str(), false),
        ] {
            let prepare = || prepare_tactical_puzzle(fen, &expected, &[]).unwrap();
            assert_eq!(
                prepare()
                    .qualify(None)
                    .unwrap()
                    .unwrap()
                    .plan(&Catalog::default())
                    .unwrap()
                    .is_none(),
                withheld,
                "{fen}"
            );

            let old = prepare().plan(&Catalog::default()).unwrap();
            let mut item = old.content.catalog.items[&old.target].clone();
            for label in ["From your games", "Renamed", ""] {
                item.label = label.into();
                assert_eq!(
                    requalify_game_tactic(&item, &pv("f5d4")).unwrap()
                        == GameTacticQualification::Withheld,
                    withheld,
                    "{fen}, {label}"
                );
            }
        }
    }

    #[test]
    fn a_game_capture_drill_compares_gains_after_defenses() {
        let equal = "7k/8/8/1r1p1r2/3N4/6p1/7P/K7 w - - 0 1";
        for answer in ["d4b5", "d4f5"] {
            let puzzle = prepare_tactical_puzzle(equal, &answer.into(), &[]).unwrap();
            // Either rook capture gains five immediately but retains only
            // four after ...gxh2. Both are equally valid ways to win material.
            let qualified = puzzle.qualify(None).unwrap().unwrap();
            assert!(qualified.plan(&Catalog::default()).unwrap().is_none());
        }

        let smaller_alternative = equal.replace("1r1p1r2", "1r1p1b2");
        let puzzle = prepare_tactical_puzzle(&smaller_alternative, &"d4b5".into(), &[]).unwrap();
        let qualified = puzzle.qualify(None).unwrap().unwrap();
        assert!(qualified.plan(&Catalog::default()).unwrap().is_some());
    }

    #[test]
    fn a_mating_capture_can_also_keep_the_material_gain() {
        let fen = "7k/6r1/5K2/8/4r1Q1/8/8/8 w - - 0 1";
        let puzzle = prepare_tactical_puzzle(fen, &"g4e4".into(), &[]).unwrap();
        let qualified = puzzle.qualify(None).unwrap().unwrap();
        assert!(qualified.plan(&Catalog::default()).unwrap().is_none());
    }

    #[test]
    fn a_qualified_line_cannot_reactivate_an_ambiguous_game_definition() {
        let fen = "8/2b5/3k4/1r1p1r2/3N4/8/8/K7 w - - 0 1";
        let root = "d4b5".into();
        let prepare = || prepare_tactical_puzzle(fen, &root, &[]).unwrap();
        let qualify = || {
            prepare()
                .qualify(Some(&pv("d4b5 d6e7 b5c7")))
                .unwrap()
                .unwrap()
        };
        let short = || prepare().qualify(None).unwrap().unwrap();
        assert!(short().plan(&Catalog::default()).unwrap().is_none());
        let legacy = prepare().plan(&Catalog::default()).unwrap();
        let fresh = qualify().plan(&Catalog::default()).unwrap().unwrap();
        assert_eq!(
            fresh.content.catalog.items[&fresh.target]
                .definition
                .decisions
                .len(),
            2
        );
        let retained_longer = short().plan(&fresh.content.catalog).unwrap().unwrap();
        assert_eq!(retained_longer.target, fresh.target);
        assert_eq!(retained_longer.content.catalog, fresh.content.catalog);
        let mut catalog = legacy.content.catalog;
        let item = catalog.items.get_mut(&legacy.target).unwrap();
        item.status = crate::ContentStatus::Quarantined;
        let proof = recognizable_payoff(fen, &root, &pv("d4b5"))
            .unwrap()
            .unwrap();
        assert!(qualify().plan(&catalog).unwrap().is_none());
        catalog
            .items
            .get_mut(&legacy.target)
            .unwrap()
            .verified_tactic = proof.motifs;
        assert!(qualify().plan(&catalog).unwrap().is_none());

        let authored = catalog.items.get_mut(&legacy.target).unwrap();
        authored.game_capture = Some(crate::GameCapture {
            capture_id: "independent-capture".into(),
        });
        let approved = qualify().plan(&catalog).unwrap().unwrap();
        let authored_short = short().plan(&catalog).unwrap().unwrap();
        assert_eq!(approved.target, legacy.target);
        assert_eq!(authored_short.target, legacy.target);
        assert_eq!(authored_short.content.catalog, approved.content.catalog);
        assert_eq!(
            approved.content.catalog.items[&legacy.target].definition,
            catalog.items[&legacy.target].definition
        );
    }

    #[test]
    fn checking_fork_reaches_bishop_and_rescan_retains_the_target() {
        let root = "d1h5".into();
        let prepare = || prepare_tactical_puzzle(FORK, &root, &[]).unwrap();
        let fresh = prepare()
            .qualify(Some(&pv("d1h5 g7g6 h5c5 g8f6")))
            .unwrap()
            .unwrap()
            .plan(&Catalog::default())
            .unwrap()
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
        assert!(
            prepared()
                .qualify(Some(&pv("d1h5 a7a3")))
                .unwrap()
                .is_none()
        );
        let mut authored = baseline.content.catalog.items[&baseline.target].clone();
        authored.provenance.push("tactic:70676e:31".into());
        assert!(!crate::game_generated_definition(&authored));
        authored.target = tempo_types::TargetId::from("tactic:70676e:31");
        let authored_catalog = Catalog {
            items: [(authored.target.clone(), authored.clone())].into(),
            lines: vec![],
        };
        let repeat = prepared()
            .qualify(Some(&pv("d1h5 g7g6 h5c5 g8f6")))
            .unwrap()
            .unwrap()
            .plan(&authored_catalog)
            .unwrap()
            .unwrap();
        assert_eq!(repeat.target, authored.target);
        assert_eq!(
            repeat.content.catalog.items[&repeat.target].definition,
            authored.definition
        );
    }

    #[test]
    fn pictured_bishop_fork_finishes_at_the_rook() {
        let fen = "4r1k1/B1p1P3/7p/1rpp1bp1/5p2/1B4P1/PPP4P/R3Q1K1 w - - 0 1";
        let variation = pv("b3d5 g8h7 d5c6 b5b4 c6e8 b4e4 g3f4 e4e6 e8c6 e6e1 a1e1");
        let proof = recognizable_payoff(fen, &"b3d5".into(), &variation)
            .unwrap()
            .unwrap();
        assert_eq!(proof.line.last().unwrap().uci, "c6e8");
        assert_eq!(proof.line.len(), 5);
        assert_eq!(
            proof.motifs.iter().map(|p| p.motif).collect::<Vec<_>>(),
            [Motif::HangingMaterial, Motif::Fork]
        );
    }

    #[test]
    fn a_discovered_check_must_enable_its_capture() {
        let independent = "n3k3/8/8/8/8/8/4B3/R3R2K w - - 0 1";
        assert!(
            recognizable_payoff(independent, &"a1a8".into(), &pv("a1a8"))
                .unwrap()
                .is_some()
        );
        assert!(
            recognizable_payoff(independent, &"e2d3".into(), &pv("e2d3 e8d7 a1a8"))
                .unwrap()
                .is_none()
        );

        let enabled = "4k3/7n/8/8/8/8/4B3/4R2K w - - 0 1";
        let proof = recognizable_payoff(enabled, &"e2d3".into(), &pv("e2d3 e8d7 d3h7"))
            .unwrap()
            .unwrap();
        assert_eq!(proof.motifs[0].motif, Motif::DiscoveredCheck);
        assert_eq!(proof.line.last().unwrap().uci, "d3h7");
    }

    #[test]
    fn a_check_does_not_join_a_fork_already_available() {
        let fen = "n6k/8/3n1n2/8/4P3/8/8/RK6 w - - 0 1";
        let separate = pv("e4e5 d6c4 e5f6");
        let fork = recognizable_payoff(fen, &"e4e5".into(), &separate)
            .unwrap()
            .unwrap();
        assert_eq!(fork.motifs[0].motif, Motif::Fork);
        let chain = pv("a1a8 h8h7 e4e5 d6c4 e5f6");
        let proof = recognizable_payoff(fen, &"a1a8".into(), &chain)
            .unwrap()
            .unwrap();
        assert_eq!(proof.line, [ChessAnswer::from("a1a8")]);
    }

    #[test]
    fn connected_tactics_can_take_more_than_three_learner_moves() {
        // Qxc4+ brings the king to b2, exposing the queen to a rook pin.
        // Taking the queen checks again and leads directly to a queen fork.
        let fen = "2r2rk1/1Q1R1ppp/4q3/8/2P5/8/P4P1P/2K4R b - - 0 1";
        let variation = pv("e6c4 c1b2 c8b8 h1c1 b8b7 d7b7 c4d4 c1c3 d4f2");
        let proof = recognizable_payoff(fen, &"e6c4".into(), &variation)
            .unwrap()
            .unwrap();
        assert_eq!(proof.line.len(), 9);
        assert_eq!(proof.motifs.last().unwrap().motif, Motif::Fork);
        let plan = prepare_tactical_puzzle(fen, &"e6c4".into(), &[])
            .unwrap()
            .qualify(Some(&variation))
            .unwrap()
            .unwrap()
            .plan(&Catalog::default())
            .unwrap()
            .unwrap();
        let item = &plan.content.catalog.items[&plan.target];
        use tempo_study::Grader;
        let mut state = crate::ChessState::at(0);
        for (i, uci) in ["e6c4", "c8b8", "b8b7", "c4d4", "d4f2"].iter().enumerate() {
            let result = crate::LegalSequenceGrader
                .answer(&item.definition, &state, &(*uci).into())
                .unwrap();
            assert!(result.feedback.correct);
            assert_eq!(result.complete, i == 4);
            state = result.state;
        }
    }

    #[test]
    fn loose_pawn_claim_fails_when_another_capture_erases_the_gain() {
        // The queen can take h3, but White can immediately take the bishop
        // on b2. The prior display clue remains appropriate for that isolated
        // capture on an authored card, whose line is supplied by its author.
        let result = prepare_tactical_puzzle(DEFENDED, &"h4h3".into(), &[])
            .unwrap()
            .qualify(Some(&pv("h4h3 d1e2")))
            .unwrap();
        assert!(result.is_none());
        assert!(
            matches!(crate::explain_tactic_move(DEFENDED, &"h4h3".into()).unwrap(),
                Some(TacticExplanation::HangingPiece{captured}) if captured.square=="h3")
        );
    }

    #[test]
    fn a_mating_sequence_explains_the_setup_without_disclosing_its_finish() {
        let fen = "8/5Q2/k2K4/8/8/8/8/8 w - - 0 1";
        let plan = prepare_tactical_puzzle(fen, &"d6c5".into(), &[])
            .unwrap()
            .qualify(Some(&pv("d6c5 a6a5 f7a2")))
            .unwrap()
            .unwrap()
            .plan(&Catalog::default())
            .unwrap()
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
    fn a_visually_available_fork_capture_can_fail_to_cash_in() {
        let fen = "4r1k1/B1p1P3/7p/1rpp1bp1/5p2/1B4P1/PPP4P/R3Q1K1 w - - 0 1";
        // After ...Rxe7, Bxb5 loses the queen to ...Rxe1+.
        let line = pv("b3d5 g8h7 d5c6 e8e7 c6b5");
        let proof = recognizable_payoff(fen, &"b3d5".into(), &line)
            .unwrap()
            .unwrap();
        assert_eq!(proof.line, [ChessAnswer::from("b3d5")]);
    }

    #[test]
    fn positional_engine_continuation_does_not_supply_a_material_payoff() {
        let fen = "r1b1r1k1/ppp2pp1/5n2/5P1q/2BN3p/2PQ3P/PP4P1/R4RK1 w - - 0 1";
        let line = pv("d4b5 e8f8 b5c7 a8b8 a1e1 c8d7 c7d5 f6d5 c4d5");
        assert_eq!(
            recognizable_payoff(fen, &"d4b5".into(), &line).unwrap(),
            None
        );
    }

    #[test]
    fn checking_capture_can_chain_directly_into_mate() {
        let fen = "8/8/8/4r3/5Q2/K7/3p4/k7 w - - 0 1";
        assert_eq!(
            tactical_payoff(fen, &"f4e5".into(), &pv("f4e5 a1b1 e5b2"))
                .unwrap()
                .unwrap(),
            pv("f4e5 a1b1 e5b2")
                .iter()
                .map(|uci| ChessAnswer::from(uci.as_str()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_mating_tail_does_not_admit_an_unreplayable_early_mate() {
        let fen = "8/8/8/4r3/5Q2/K7/8/k2N4 w - - 0 1";
        let board = parse_position(fen, true).unwrap();
        assert!(mate_moves(&board).contains(&ChessAnswer::from("f4c1")));
        let line = pv("f4e5 a1b1 e5b2");
        assert_eq!(
            tactical_payoff(fen, &"f4e5".into(), &line)
                .unwrap()
                .unwrap(),
            [ChessAnswer::from("f4e5")]
        );
    }

    #[test]
    fn exchanges_must_gain_from_the_starting_balance() {
        // White gives a rook for a pawn, then wins the recapturing queen.
        let fen = "3q3k/3p4/8/8/6B1/7P/8/3R3K w - - 0 1";
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
