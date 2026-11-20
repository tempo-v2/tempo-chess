//! A detector implemented entirely against the public Chess contract.

use tempo_chess::{
    BoardSubject, DetectedMoment, DetectionControl, DetectionOutcome, DetectionStop,
    DetectionValidationError, DetectorDescriptor, GameDetectionInput, GameDetector, LearnerColor,
    PieceRole, parse_played_games, validate_detected_moments,
};

fn game_input(pgn: &str, learner: LearnerColor) -> GameDetectionInput {
    let mut games = parse_played_games(pgn).unwrap();
    assert_eq!(games.len(), 1);
    GameDetectionInput::new(games.pop().unwrap().unwrap().timeline(), learner)
}

#[test]
fn played_game_timeline_retains_black_custom_start_and_special_moves() {
    for (fen, pgn, actual, final_fen, first_side) in [
        (
            "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 7 22",
            "22... O-O-O 23. O-O 0-1",
            ["e8c8", "e1g1"],
            "2kr3r/8/8/8/8/8/8/R4RK1 b - - 9 23",
            LearnerColor::Black,
        ),
        (
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2",
            "2. exd6 Kf7 1-0",
            ["e5d6", "e8f7"],
            "8/5k2/3P4/8/8/8/8/4K3 w - - 1 3",
            LearnerColor::White,
        ),
        (
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            "1. a8=N Ke7 1-0",
            ["a7a8n", "e8e7"],
            "N7/4k3/8/8/8/8/8/4K3 w - - 1 2",
            LearnerColor::White,
        ),
    ] {
        let input = game_input(
            &format!("[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{pgn}"),
            LearnerColor::White,
        );
        let boards = input.timeline().occurrences();
        assert_eq!(
            boards
                .iter()
                .map(|board| board.offset())
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(boards[0].fen(), fen);
        assert_eq!(boards[0].side_to_move(), first_side);
        assert_ne!(boards[1].side_to_move(), first_side);
        assert_eq!(boards[2].fen(), final_fen);
        for (board, uci) in boards.iter().zip(actual) {
            assert_eq!(board.next_move().unwrap().uci, uci);
        }
        assert!(boards[2].next_move().is_none());
    }
}

#[test]
fn played_game_timeline_keeps_every_repeated_occurrence_past_retired_limits() {
    let pgn = format!("{} 1/2-1/2", "Nf3 Nf6 Ng1 Ng8 ".repeat(130));
    let input = game_input(&pgn, LearnerColor::White);
    let boards = input.timeline().occurrences();
    assert_eq!(boards.len(), 521);
    assert_eq!(boards[0].position(), boards[520].position());
    assert_ne!(boards[0].offset(), boards[520].offset());
    assert_ne!(boards[0].fen(), boards[520].fen());
    assert!(boards[520].next_move().is_none());
}

#[test]
fn game_detector_input_identity_tracks_only_semantic_input() {
    let white = LearnerColor::White;
    let base = game_input("1. e4 e5 1-0", white);
    let equivalent = game_input(
        "[Event \"Changed display tag\"]\n\n1. e4! {comment} (1. d4 d5) e5 0-1",
        white,
    );
    assert_eq!(base.timeline(), equivalent.timeline());
    assert_eq!(base.fingerprint(), equivalent.fingerprint());
    assert_ne!(
        base.fingerprint(),
        game_input("1. e4 c5 1-0", white).fingerprint()
    );
    assert_ne!(
        base.fingerprint(),
        game_input("1. e4 e5 1-0", LearnerColor::Black).fingerprint()
    );
    assert_ne!(
        base.fingerprint(),
        game_input(
            "[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 6 3\"]\n\n3. e4 e5 1-0",
            white
        )
        .fingerprint()
    );
}

struct ExternalDetector;

impl GameDetector for ExternalDetector {
    fn descriptor(&self) -> DetectorDescriptor {
        DetectorDescriptor {
            id: "test.detector".into(),
            revision: "1".into(),
            configuration_fingerprint: "fixture-v1".into(),
            label: "Test learner move".into(),
        }
    }

    fn detect(
        &self,
        input: &GameDetectionInput,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        let mut moments = Vec::new();
        for occurrence in input.timeline().occurrences() {
            if let Err(stop) = control.check() {
                return DetectionOutcome::Stopped(stop);
            }
            if occurrence.side_to_move() == input.learner() {
                if let Some(actual) = occurrence.next_move() {
                    moments.push(DetectedMoment {
                        occurrence: occurrence.offset(),
                        code: "learner_move_played".into(),
                        subjects: vec![],
                        candidate_move: Some(actual.uci.clone()),
                        explanation: "The learner played a move from this board.".into(),
                        tactical: None,
                    });
                }
            }
        }
        DetectionOutcome::Complete(moments)
    }
}

struct Check(Result<(), DetectionStop>);

impl DetectionControl for Check {
    fn check(&self) -> Result<(), DetectionStop> {
        self.0
    }
}

#[test]
fn game_detector_external_fake_observes_the_played_move_and_checks_for_stop() {
    let detector: &dyn GameDetector = &ExternalDetector;
    let input = game_input("1. e4 c5 1-0", LearnerColor::Black);
    assert_eq!(detector.descriptor().validate(), Ok(()));
    let DetectionOutcome::Complete(findings) = detector.detect(&input, &Check(Ok(()))) else {
        panic!("expected completed test observation");
    };
    let validated = validate_detected_moments(&input, findings).unwrap();
    let [finding] = validated.as_slice() else {
        panic!("expected exactly one learner move");
    };
    let anchor = &input.timeline().occurrences()[1];
    assert_eq!(finding.occurrence, 1);
    assert_eq!(finding.candidate_move.as_ref().unwrap().uci, "c7c5");
    assert_eq!(finding.candidate_san.as_deref(), Some("c5"));
    assert_eq!(finding.side_to_move, LearnerColor::Black);
    assert_eq!(finding.fen, anchor.fen());
    assert_eq!(&finding.position, anchor.position());
    for stop in [DetectionStop::Cancelled, DetectionStop::Deadline] {
        assert_eq!(
            detector.detect(&input, &Check(Err(stop))),
            DetectionOutcome::Stopped(stop)
        );
    }
    let only_opponent = game_input("1. e4 1-0", LearnerColor::Black);
    assert_eq!(
        detector.detect(&only_opponent, &Check(Ok(()))),
        DetectionOutcome::Complete(vec![])
    );
    struct StopAfterOneMove(std::cell::Cell<usize>);

    impl DetectionControl for StopAfterOneMove {
        fn check(&self) -> Result<(), DetectionStop> {
            let checks = self.0.get();
            self.0.set(checks + 1);
            if checks < 2 {
                Ok(())
            } else {
                Err(DetectionStop::Deadline)
            }
        }
    }

    // Even if the fake has accumulated one finding, it reports only the stop.
    assert_eq!(
        detector.detect(&input, &StopAfterOneMove(std::cell::Cell::new(0))),
        DetectionOutcome::Stopped(DetectionStop::Deadline)
    );
}

#[test]
fn game_detector_rejects_invalid_claims_and_normalizes_finding_identity() {
    use DetectionValidationError::*;
    use LearnerColor::White;
    let input = game_input("1. Nf3 Nf6 2. Ng1 Ng8 1/2-1/2", White);
    let valid = DetectedMoment {
        occurrence: 0,
        code: "test".into(),
        subjects: vec![
            BoardSubject {
                square: "g1".into(),
                role: PieceRole::Knight,
                color: White,
            },
            BoardSubject {
                square: "e2".into(),
                role: PieceRole::Pawn,
                color: White,
            },
        ],
        candidate_move: Some("g1f3".into()),
        explanation: "original wording".into(),
        tactical: None,
    };
    let mut equivalent = valid.clone();
    equivalent.subjects.reverse();
    equivalent.explanation = "new wording".into();
    let mut repeated = valid.clone();
    repeated.occurrence = 4;
    let fingerprint =
        |moment| validate_detected_moments(&input, vec![moment]).unwrap()[0].semantic_fingerprint();
    assert_eq!(fingerprint(valid.clone()), fingerprint(equivalent.clone()));
    assert_ne!(fingerprint(valid.clone()), fingerprint(repeated));
    assert_eq!(
        validate_detected_moments(&input, vec![valid.clone(), equivalent]),
        Err(DuplicateFinding)
    );
    for (update, error) in [
        (|m: &mut DetectedMoment| m.occurrence = 5, InvalidOccurrence),
        (|m: &mut DetectedMoment| m.code = " ".into(), InvalidCode),
        (
            |m: &mut DetectedMoment| m.subjects[0].square = "z9".into(),
            InvalidSubject,
        ),
        (
            |m: &mut DetectedMoment| m.subjects[0].square = "f3".into(),
            InvalidSubject,
        ),
        (
            |m: &mut DetectedMoment| m.subjects[0].role = PieceRole::King,
            InvalidSubject,
        ),
        (
            |m: &mut DetectedMoment| m.subjects[0].color = LearnerColor::Black,
            InvalidSubject,
        ),
        (
            |m: &mut DetectedMoment| m.candidate_move = Some("a1a4".into()),
            InvalidCandidateMove,
        ),
    ] as [(fn(&mut DetectedMoment), DetectionValidationError); 7]
    {
        let mut invalid = valid.clone();
        update(&mut invalid);
        assert_eq!(
            validate_detected_moments(&input, vec![valid.clone(), invalid]),
            Err(error)
        );
    }
    let mut descriptor = ExternalDetector.descriptor();
    descriptor.revision.clear();
    assert_eq!(descriptor.validate(), Err(InvalidDescriptor));
}
