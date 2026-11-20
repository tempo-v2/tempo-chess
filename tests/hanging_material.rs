//! Public detector and grader consumers; no database or engine executable.

use std::sync::{Arc, Mutex};
use tempo_chess::*;
use tempo_study::Grader;
use tempo_types::{ContentVersion, Grade};

const WHITE: &str = "7k/n7/8/8/8/8/8/R5K1 w - - 0 1";

struct Continue;
impl DetectionControl for Continue {
    fn check(&self) -> Result<(), DetectionStop> {
        Ok(())
    }
}

type Requests = Arc<Mutex<Vec<TacticalEvaluationRequest>>>;
type ScoreFunction = dyn Fn(&TacticalEvaluationRequest, usize) -> Result<RootEvaluation, EvaluationFailure>
    + Send
    + Sync;
struct Fake {
    requests: Requests,
    score: Box<ScoreFunction>,
}
impl TacticalEvaluator for Fake {
    fn descriptor(&self) -> EvaluationProvenance {
        provenance()
    }
    fn evaluate(
        &self,
        request: &TacticalEvaluationRequest,
        _: &dyn DetectionControl,
    ) -> Result<RootEvaluation, EvaluationFailure> {
        let mut seen = self.requests.lock().unwrap();
        let index = seen.len();
        seen.push(request.clone());
        (self.score)(request, index)
    }
}

fn provenance() -> EvaluationProvenance {
    EvaluationProvenance {
        engine: "Test evaluator".into(),
        engine_version: "1".into(),
        configuration_fingerprint: "all-roots".into(),
    }
}

fn roots(
    request: &TacticalEvaluationRequest,
    chosen: &[(&str, EvaluationScore)],
) -> RootEvaluation {
    RootEvaluation {
        provenance: provenance(),
        depth: 12,
        nodes: 10_000,
        roots: request
            .legal_moves
            .iter()
            .map(|uci| RootScore {
                uci: uci.clone(),
                score: chosen
                    .iter()
                    .find(|(mv, _)| *mv == uci)
                    .map_or(EvaluationScore::Centipawns(0), |(_, score)| *score),
                bound: ScoreBound::Exact,
            })
            .collect(),
    }
}

fn input(fen: &str, line: &str, learner: LearnerColor) -> GameDetectionInput {
    let game = parse_played_games(&format!("[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{line}"))
        .unwrap()
        .pop()
        .unwrap()
        .unwrap();
    GameDetectionInput::new(game.timeline(), learner)
}

fn detect(
    fen: &str,
    line: &str,
    learner: LearnerColor,
    score: impl Fn(&TacticalEvaluationRequest, usize) -> Result<RootEvaluation, EvaluationFailure>
    + Send
    + Sync
    + 'static,
) -> (Vec<ValidatedMoment>, Requests) {
    let requests = Arc::new(Mutex::new(vec![]));
    let detector = HangingMaterialDetector::new(
        Fake {
            requests: requests.clone(),
            score: Box::new(score),
        },
        HangingMaterialConfig::default(),
    )
    .unwrap();
    let input = input(fen, line, learner);
    let DetectionOutcome::Complete(findings) = detector.detect(&input, &Continue) else {
        panic!("complete detector result expected");
    };
    (
        validate_detected_moments(&input, findings).unwrap(),
        requests,
    )
}

fn candidates(fen: &str) -> Vec<String> {
    hanging_captures(fen)
        .unwrap()
        .into_iter()
        .map(|c| c.answer.uci)
        .collect()
}

#[test]
fn candidates_obey_legal_pins_xrays_checks_and_king_safety() {
    assert!(candidates(WHITE).contains(&"a1a7".into()));
    // A knight defending f8 cannot move away from the rook's pin on its king.
    assert!(candidates("3k1r2/3n2B1/8/8/8/8/8/3R3K w - - 0 1").contains(&"g7f8".into()));
    // An attacker pinned to its own king cannot collect the apparently free knight.
    assert!(!candidates("4r2k/8/8/8/8/8/n3R3/4K3 w - - 0 1").contains(&"e2a2".into()));
    // The rook behind the knight can legally recapture after it is removed.
    assert!(!candidates("r6k/n7/8/8/8/8/8/R5K1 w - - 0 1").contains(&"a1a7".into()));
    // A king cannot take a defended pawn, and an irrelevant capture cannot ignore check.
    assert!(!candidates("k5b1/8/8/3p4/4K3/8/8/8 w - - 0 1").contains(&"e4d5".into()));
    assert!(!candidates("4r2k/8/8/8/8/8/2n5/3BK3 w - - 0 1").contains(&"d1c2".into()));
}

#[test]
fn candidates_include_black_en_passant_and_each_capture_promotion() {
    assert!(candidates("r5k1/8/8/8/8/8/N7/7K b - - 0 1").contains(&"a8a2".into()));
    let ep = hanging_captures("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
    let ep = ep.iter().find(|c| c.answer.uci == "e5d6").unwrap();
    assert_eq!(ep.target.square, "d5");
    assert_eq!(ep.target.role, PieceRole::Pawn);
    let promotions = candidates("1r5k/P7/8/8/8/8/8/7K w - - 0 1");
    for suffix in ['b', 'n', 'r', 'q'] {
        assert!(promotions.contains(&format!("a7b8{suffix}")));
    }
    assert!(!promotions.iter().any(|mv| mv.starts_with("a7a8")));
}

#[test]
fn reports_every_actual_learner_opportunity_and_verifies_missed_puzzle_separately() {
    let (findings, seen) = detect(
        WHITE,
        "1. Kg2 Kg7 0-1",
        LearnerColor::White,
        |request, _| {
            Ok(roots(
                request,
                &[("a1a7", EvaluationScore::Centipawns(1000))],
            ))
        },
    );
    assert_eq!(findings.len(), 1); // The final White-to-move board is not a decision.
    let fact = findings[0].tactical.as_ref().unwrap();
    assert_eq!(fact.outcome, TacticalOutcome::MissedGoodCapture);
    assert_eq!(fact.played.uci, "g1g2");
    assert_eq!(
        fact.puzzle.as_ref().unwrap().answer,
        ChessAnswer::from("a1a7")
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(matches!(
        fact.standalone_analysis,
        Some(EvaluationStatus::Complete { .. })
    ));

    let (black, _) = detect(
        "r5k1/8/8/8/8/8/N7/7K b - - 0 1",
        "1... Rxa2 2. Kg1 0-1",
        LearnerColor::Black,
        |request, _| {
            Ok(roots(
                request,
                &[("a8a2", EvaluationScore::Centipawns(1000))],
            ))
        },
    );
    assert_eq!(
        black[0].tactical.as_ref().unwrap().outcome,
        TacticalOutcome::TookGoodCapture
    );
}

#[test]
fn equivalent_capture_or_quiet_alternative_is_never_a_missed_opportunity() {
    for (line, expected) in [
        ("1. Rxa7 Kg8 1-0", TacticalOutcome::TookGoodCapture),
        ("1. Kg2 Kg7 0-1", TacticalOutcome::GoodAlternative),
    ] {
        let (findings, seen) = detect(WHITE, line, LearnerColor::White, |request, _| {
            Ok(roots(
                request,
                &[
                    ("a1a7", EvaluationScore::Centipawns(1000)),
                    ("g1g2", EvaluationScore::Centipawns(980)),
                ],
            ))
        });
        let fact = findings[0].tactical.as_ref().unwrap();
        assert_eq!(fact.outcome, expected);
        assert_eq!(
            fact.accepted_moves,
            vec![ChessAnswer::from("a1a7"), ChessAnswer::from("g1g2")]
        );
        assert!(fact.puzzle.is_none());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[test]
fn poisoned_capture_is_comparison_evidence_without_a_puzzle() {
    for (line, expected) in [
        ("1. Rxa7 Kg8 1-0", TacticalOutcome::TookInferiorCapture),
        ("1. Kg2 Kg7 0-1", TacticalOutcome::DeclinedInferiorCapture),
    ] {
        let (findings, _) = detect(WHITE, line, LearnerColor::White, |request, _| {
            Ok(roots(
                request,
                &[
                    ("a1a7", EvaluationScore::Centipawns(-700)),
                    ("g1g2", EvaluationScore::Centipawns(100)),
                ],
            ))
        });
        let fact = findings[0].tactical.as_ref().unwrap();
        assert_eq!(fact.outcome, expected);
        assert_eq!(fact.candidates[0].quality, CaptureQuality::Inferior);
        assert!(fact.puzzle.is_none());
    }
}

#[test]
fn incomplete_bounds_wrong_provenance_and_unavailable_scores_are_uncertain() {
    for mode in 0..5 {
        let (findings, _) = detect(
            WHITE,
            "1. Kg2 Kg7 0-1",
            LearnerColor::White,
            move |request, _| {
                if mode == 4 {
                    return Err(EvaluationFailure::Unavailable);
                }
                let mut result = roots(request, &[("a1a7", EvaluationScore::Centipawns(1000))]);
                match mode {
                    0 => {
                        result.roots.pop();
                    }
                    1 => result.roots[0].bound = ScoreBound::Lower,
                    2 => result.provenance.configuration_fingerprint = "other".into(),
                    3 => result.roots[0].score = EvaluationScore::Mate(0),
                    _ => unreachable!(),
                }
                Ok(result)
            },
        );
        let fact = findings[0].tactical.as_ref().unwrap();
        assert_eq!(fact.outcome, TacticalOutcome::Uncertain);
        assert!(fact.accepted_moves.is_empty());
        assert!(fact.puzzle.is_none());
    }
}

#[test]
fn mate_scores_are_typed_and_standalone_incompatibility_blocks_false_training() {
    let (mate, calls) = detect(
        WHITE,
        "1. Kg2 Kg7 0-1",
        LearnerColor::White,
        |request, _| {
            Ok(roots(
                request,
                &[
                    ("a1a7", EvaluationScore::Mate(2)),
                    ("g1g2", EvaluationScore::Centipawns(30_000)),
                ],
            ))
        },
    );
    let result = mate[0].tactical.as_ref().unwrap();
    assert_eq!(result.outcome, TacticalOutcome::WinningMate);
    assert_eq!(result.accepted_moves, [ChessAnswer::from("a1a7")]);
    assert!(result.candidates.is_empty());
    assert!(result.puzzle.is_none());
    assert_eq!(calls.lock().unwrap().len(), 1);
    let (changed, _) = detect(
        WHITE,
        "1. Kg2 Kg7 0-1",
        LearnerColor::White,
        |request, call| {
            Ok(roots(
                request,
                &[(
                    if call == 0 { "a1a7" } else { "g1g2" },
                    EvaluationScore::Centipawns(1000),
                )],
            ))
        },
    );
    let fact = changed[0].tactical.as_ref().unwrap();
    assert_eq!(fact.outcome, TacticalOutcome::Uncertain);
    assert!(fact.puzzle.is_none());
    assert_eq!(fact.candidates[0].quality, CaptureQuality::BestOrEquivalent);
}

#[test]
fn typed_validation_rejects_forged_outcomes_and_display_text_is_irrelevant() {
    let game = input(WHITE, "1. Kg2 Kg7 0-1", LearnerColor::White);
    let detector = HangingMaterialDetector::new(
        Fake {
            requests: Default::default(),
            score: Box::new(|request, _| {
                Ok(roots(
                    request,
                    &[("a1a7", EvaluationScore::Centipawns(1000))],
                ))
            }),
        },
        HangingMaterialConfig::default(),
    )
    .unwrap();
    let DetectionOutcome::Complete(mut findings) = detector.detect(&game, &Continue) else {
        panic!();
    };
    let original = validate_detected_moments(&game, findings.clone()).unwrap();
    findings[0].explanation = "Changed display wording".into();
    assert_eq!(
        original[0].semantic_fingerprint(),
        validate_detected_moments(&game, findings.clone()).unwrap()[0].semantic_fingerprint()
    );
    findings[0].tactical.as_mut().unwrap().outcome = TacticalOutcome::TookGoodCapture;
    assert_eq!(
        validate_detected_moments(&game, findings),
        Err(DetectionValidationError::InvalidTacticalAssessment)
    );
}

#[test]
fn generated_puzzles_reuse_legacy_exact_content_and_preserve_its_version() {
    let first = plan_tactical_puzzle(
        &Catalog::default(),
        WHITE,
        &"a1a7".into(),
        &["game:one".into()],
    )
    .unwrap();
    let repeated = plan_tactical_puzzle(
        &first.content.catalog,
        &WHITE.replace("0 1", "13 55"),
        &"a1a7".into(),
        &["game:two".into()],
    )
    .unwrap();
    assert_eq!(first.target, repeated.target);
    assert_eq!(repeated.content.catalog.items.len(), 1);
    let old = &first.content.catalog.items[&first.target];
    let new = &repeated.content.catalog.items[&first.target];
    assert_eq!(old.definition, new.definition);
    assert_eq!(old.version, new.version);
    assert!(
        new.provenance.contains(&"game:one".into()) && new.provenance.contains(&"game:two".into())
    );
    assert_eq!(new.definition.scheme, GradingScheme::ExactSequenceV1);
    let state = LegalSequenceGrader.start(&new.definition).unwrap().state;
    assert_eq!(
        serde_json::to_string(&state).unwrap(),
        r#"{"decision_index":0}"#
    );
    assert_eq!(
        LegalSequenceGrader
            .answer(&new.definition, &state, &"a1a7".into())
            .unwrap()
            .assessment,
        Some(Grade::Good)
    );
    assert_eq!(
        LegalSequenceGrader
            .answer(&new.definition, &state, &"g1g2".into())
            .unwrap()
            .assessment,
        Some(Grade::Again)
    );
}

#[test]
fn engine_selected_capture_resolves_only_its_exact_import_quarantine_without_revision() {
    let source = |id: &str, answer: &str| TacticRecord {
        namespace: "legacy".into(),
        source_id: id.into(),
        solver_fen: WHITE.into(),
        solution: vec![answer.into()],
        label: id.into(),
        provenance: vec!["original collection".into()],
    };
    let original = source("retained capture", "a1a7");
    let other = source("conflicting answer", "g1g2");
    let legacy = compile_tactics(std::slice::from_ref(&original)).unwrap();
    let selected = ChessAnswer::from("a1a7");
    let provenance = vec!["game:one".into()];
    let mut established = plan_tactical_puzzle(&legacy, WHITE, &selected, &provenance)
        .unwrap()
        .content
        .catalog;
    // Exercise retention of a previously revised target, not just version one.
    established
        .items
        .get_mut(&original.target_id())
        .unwrap()
        .version = ContentVersion(7);
    let conflicted = plan_import(
        &established,
        &compile_tactics(std::slice::from_ref(&other)).unwrap(),
    )
    .unwrap()
    .catalog;
    assert!(
        conflicted
            .items
            .values()
            .all(|item| item.status == ContentStatus::Quarantined)
    );
    let before = &conflicted.items[&original.target_id()];
    let planned = plan_tactical_puzzle(&conflicted, WHITE, &selected, &provenance).unwrap();
    let retained = &planned.content.catalog.items[&planned.target];
    assert_eq!(planned.target, original.target_id());
    assert_eq!(retained.status, ContentStatus::Active);
    assert_eq!(retained.definition, before.definition);
    assert_eq!(retained.version, ContentVersion(7));
    assert_eq!(retained.label, before.label);
    assert_eq!(retained.provenance, before.provenance);
    assert_eq!(
        planned.content.changes,
        vec![ContentChange {
            target: original.target_id(),
            kind: ChangeKind::Metadata,
        }]
    );
    assert_eq!(planned.content.catalog.items.len(), 2);
    assert_eq!(
        planned.content.catalog.items[&other.target_id()],
        conflicted.items[&other.target_id()]
    );
    let repeated =
        plan_tactical_puzzle(&planned.content.catalog, WHITE, &selected, &provenance).unwrap();
    assert_eq!(repeated.content.catalog, planned.content.catalog);
    assert_eq!(repeated.content.changes[0].kind, ChangeKind::Unchanged);
    // Planning is pure: even the resolved source proposal stays quarantined.
    assert_eq!(
        conflicted.items[&original.target_id()].status,
        ContentStatus::Quarantined
    );
}

#[test]
fn two_available_captures_produce_one_puzzle_for_the_better_engine_capture() {
    let fen = "7k/n7/8/8/5r2/8/8/R1B3K1 w - - 0 1";
    let (findings, _) = detect(fen, "1. Kh2 Kg8 0-1", LearnerColor::White, |request, _| {
        Ok(roots(
            request,
            &[
                ("a1a7", EvaluationScore::Centipawns(1000)),
                ("c1f4", EvaluationScore::Centipawns(1020)),
            ],
        ))
    });
    let fact = findings[0].tactical.as_ref().unwrap();
    assert_eq!(fact.candidates.len(), 2);
    assert_eq!(fact.outcome, TacticalOutcome::MissedGoodCapture);
    let puzzle = fact.puzzle.as_ref().unwrap();
    assert_eq!(puzzle.answer, ChessAnswer::from("c1f4"));
    let plan = plan_tactical_puzzle(&Catalog::default(), &puzzle.fen, &puzzle.answer, &[]).unwrap();
    let definition = &plan.content.catalog.items[&plan.target].definition;
    assert_eq!(definition.decisions.len(), 1);
    assert_eq!(definition.decisions[0].expected, ChessAnswer::from("c1f4"));
    assert_eq!(definition.scheme, GradingScheme::ExactSequenceV1);
}
