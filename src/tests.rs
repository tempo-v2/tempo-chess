use super::*;
use std::convert::Infallible;
use tempo_study::{
    AttemptStatus, Availability, Exercise, GradingError, QuestionSource, StudyError,
};
use tempo_types::{ContentVersion, Grade, Revision, TargetId};
struct Source(Option<Exercise<ChessDefinition>>);
impl QuestionSource for Source {
    type Definition = ChessDefinition;
    type Error = Infallible;
    fn next(&mut self) -> Result<Availability<Exercise<ChessDefinition>>, Infallible> {
        Ok(self
            .0
            .take()
            .map(Availability::Ready)
            .unwrap_or(Availability::Empty))
    }
}
fn tactic() -> (ChessDefinition, ChessAttempt) {
    let catalog = demo_catalog();
    let exercise = catalog.items[&TargetId::from("t1")].exercise();
    let definition = exercise.definition.clone();
    let Availability::Ready(started) = tempo_study::next(
        &mut Source(Some(exercise)),
        &ExactSequenceGrader,
        "dataset".into(),
        "a1".into(),
    )
    .unwrap() else {
        panic!("fixture ready")
    };
    (definition, started.attempt)
}
#[test]
fn clean_tactic_assesses_only_after_both_learner_decisions() {
    let (definition, attempt) = tactic();
    let first = tempo_study::submit(
        &ExactSequenceGrader,
        &definition,
        &attempt,
        Revision(0),
        &"e2e4".into(),
    )
    .unwrap();
    assert_eq!(first.assessment, None);
    assert_eq!(first.feedback.autoplay, vec![ChessAnswer::from("e7e5")]);
    assert_eq!(first.attempt.prompt.as_ref().unwrap().fen, SECOND_FEN);
    let last = tempo_study::submit(
        &ExactSequenceGrader,
        &definition,
        &first.attempt,
        Revision(1),
        &"g1f3".into(),
    )
    .unwrap();
    assert_eq!(last.assessment, Some(Grade::Good));
    assert_eq!(last.attempt.status, AttemptStatus::Complete);
}
#[test]
fn miss_on_either_decision_is_sticky_and_correction_is_not_credit() {
    for miss_index in [0, 1] {
        let (definition, mut attempt) = tactic();
        let mut emitted = Vec::new();
        for (index, correct) in ["e2e4", "g1f3"].into_iter().enumerate() {
            if index == miss_index {
                let miss = tempo_study::submit(
                    &ExactSequenceGrader,
                    &definition,
                    &attempt,
                    attempt.revision,
                    &"a2a3".into(),
                )
                .unwrap();
                emitted.extend(miss.assessment);
                attempt = miss.attempt;
            }
            let next = tempo_study::submit(
                &ExactSequenceGrader,
                &definition,
                &attempt,
                attempt.revision,
                &correct.into(),
            )
            .unwrap();
            emitted.extend(next.assessment);
            attempt = next.attempt;
        }
        assert_eq!(emitted, vec![Grade::Again]);
        assert_eq!(attempt.terminal, Some(Grade::Again));
        assert_eq!(attempt.status, AttemptStatus::Complete);
    }
}
#[test]
fn invalid_answer_is_a_protocol_error_and_does_not_grade() {
    let (definition, attempt) = tactic();
    assert!(matches!(
        tempo_study::submit(
            &ExactSequenceGrader,
            &definition,
            &attempt,
            Revision(0),
            &"queen takes".into()
        ),
        Err(StudyError::Grading(GradingError::InvalidAnswer(_)))
    ));
    assert_eq!(attempt.revision, Revision(0));
    assert_eq!(attempt.terminal, None);
}
#[test]
fn metadata_preserves_version_while_response_change_versions_only_target() {
    let current = demo_catalog();
    let mut incoming = current.clone();
    incoming
        .items
        .get_mut(&TargetId::from("r1:p1"))
        .unwrap()
        .label = "Annotation updated".into();
    let metadata = plan_import(&current, &incoming).unwrap();
    assert_eq!(
        metadata.catalog.items[&TargetId::from("r1:p1")].version,
        ContentVersion(1)
    );
    assert!(metadata.changes.contains(&ContentChange {
        target: "r1:p1".into(),
        kind: ChangeKind::Metadata
    }));
    incoming
        .items
        .get_mut(&TargetId::from("r1:p1"))
        .unwrap()
        .definition
        .decisions[0]
        .expected = "d2d4".into();
    let semantic = plan_import(&current, &incoming).unwrap();
    assert_eq!(
        semantic.catalog.items[&TargetId::from("r1:p1")].version,
        ContentVersion(2)
    );
    assert_eq!(
        semantic.catalog.items[&TargetId::from("r1:p2")].version,
        ContentVersion(1)
    );
    assert!(semantic.changes.contains(&ContentChange {
        target: "r1:p1".into(),
        kind: ChangeKind::Semantic
    }));
}
#[test]
fn repetitions_share_target_and_conflicting_second_target_is_rejected() {
    let mut current = demo_catalog();
    current.lines[0].targets.push("r1:p1".into());
    current.validate().unwrap();
    let mut duplicate = current.items[&TargetId::from("r1:p1")].clone();
    duplicate.target = "r1:competing".into();
    duplicate.definition.decisions[0].expected = "d2d4".into();
    let incoming = Catalog {
        items: [(duplicate.target.clone(), duplicate)].into(),
        lines: vec![],
    };
    assert!(matches!(
        plan_import(&current, &incoming),
        Err(ChessError::ConflictingResponse { .. })
    ));
}
#[test]
fn duplicate_tactic_adds_provenance_and_distinct_same_start_is_quarantined() {
    let current = demo_catalog();
    let mut duplicate = current.items[&TargetId::from("t1")].clone();
    duplicate.target = "imported-id".into();
    duplicate.provenance = vec!["another source".into()];
    let mut incoming = Catalog {
        items: [(duplicate.target.clone(), duplicate)].into(),
        lines: vec![],
    };
    let merged = plan_import(&current, &incoming).unwrap();
    assert_eq!(merged.catalog.items.len(), current.items.len());
    assert!(
        merged.catalog.items[&TargetId::from("t1")]
            .provenance
            .contains(&"another source".into())
    );
    let changed = incoming
        .items
        .get_mut(&TargetId::from("imported-id"))
        .unwrap();
    changed.definition.decisions[1].expected = "f1c4".into();
    let conflicted = plan_import(&current, &incoming).unwrap();
    assert_eq!(
        conflicted.catalog.items[&TargetId::from("t1")].status,
        ContentStatus::Quarantined
    );
    assert_eq!(
        conflicted.catalog.items[&TargetId::from("imported-id")].status,
        ContentStatus::Quarantined
    );
    assert_eq!(
        conflicted.catalog.items[&TargetId::from("r1:p1")].status,
        ContentStatus::Active
    );
}
#[test]
fn fixture_ports_reject_unknown_inputs_instead_of_simulating_real_parsing() {
    let parser = demo_parser();
    let known = parser
        .parse(&ImportRequest {
            pgn: REPERTOIRE_PGN.into(),
            source: "test".into(),
            repertoire: Some("r1".into()),
        })
        .unwrap();
    assert_eq!(known.items.len(), 2);
    assert!(
        parser
            .parse(&ImportRequest {
                pgn: "1. a3 *".into(),
                source: "test".into(),
                repertoire: None
            })
            .is_err()
    );
    assert!(demo_canonicalizer().canonicalize("unknown FEN").is_err());
    assert!(
        demo_comparator()
            .compare("unknown game", &demo_catalog())
            .is_err()
    );
    assert_eq!(
        demo_comparator()
            .compare(GAME_PGN, &demo_catalog())
            .unwrap()[0]
            .target,
        TargetId::from("r1:p1")
    );
}
