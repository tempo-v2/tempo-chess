//! One concrete detector using the existing game-detector contract.

use crate::standard_position_canonicalizer::parse_position;
use crate::{
    BoardSubject, ChessAnswer, ChessError, DetectedMoment, DetectionControl, DetectionOutcome,
    DetectionStop, DetectorDescriptor, EvaluationFailure, EvaluationProvenance, EvaluationScore,
    EvaluationStatus, GameDetectionInput, GameDetector, LearnerColor, PieceRole, PositionPass,
    RootEvaluation, ScoreBound, TacticalEvaluationRequest, TacticalEvaluator,
};
use cozy_chess::{Board, Move, Piece, Square, util::display_uci_move};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TacticalKind {
    HangingMaterial,
    WinningMate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangingCapture {
    pub answer: ChessAnswer,
    pub target: BoardSubject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureQuality {
    BestOrEquivalent,
    Inferior,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangingCaptureAssessment {
    pub capture: HangingCapture,
    pub quality: CaptureQuality,
    pub score: Option<EvaluationScore>,
}

/// Observed play only. These outcomes do not claim what the learner noticed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TacticalOutcome {
    TookGoodCapture,
    GoodAlternative,
    MissedGoodCapture,
    TookInferiorCapture,
    DeclinedInferiorCapture,
    Uncertain,
    WinningMate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TacticalPuzzleSolution {
    pub fen: String,
    pub answer: ChessAnswer,
}

/// Typed evidence for one actual learner decision. Game-quality comparisons and
/// the standalone puzzle remain separate because repetition/draw context can
/// change which move is good. Only a validated missed-good result has a puzzle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TacticalAssessment {
    pub kind: TacticalKind,
    pub candidates: Vec<HangingCaptureAssessment>,
    pub played: ChessAnswer,
    pub outcome: TacticalOutcome,
    pub analysis: EvaluationStatus,
    pub standalone_analysis: Option<EvaluationStatus>,
    pub accepted_moves: Vec<ChessAnswer>,
    pub puzzle: Option<TacticalPuzzleSolution>,
    pub equivalence_cp: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HangingMaterialConfig {
    pub equivalence_cp: i32,
}

impl Default for HangingMaterialConfig {
    fn default() -> Self {
        Self { equivalence_cp: 30 }
    }
}

pub struct HangingMaterialDetector<E> {
    evaluator: E,
    provenance: EvaluationProvenance,
    config: HangingMaterialConfig,
}

impl<E: TacticalEvaluator> HangingMaterialDetector<E> {
    pub fn new(evaluator: E, config: HangingMaterialConfig) -> Result<Self, ChessError> {
        let provenance = evaluator.descriptor();
        if config.equivalence_cp < 0
            || [
                &provenance.engine,
                &provenance.engine_version,
                &provenance.configuration_fingerprint,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            return Err(ChessError::InvalidContent(
                "invalid tactical evaluation configuration".into(),
            ));
        }
        Ok(Self {
            evaluator,
            provenance,
            config,
        })
    }

    fn evaluate(
        &self,
        request: &TacticalEvaluationRequest,
        control: &dyn DetectionControl,
    ) -> Result<EvaluationStatus, DetectionStop> {
        control.check()?;
        let result = self.evaluator.evaluate(request, control);
        control.check()?;
        Ok(match result {
            Ok(evaluation) if complete_evaluation(request, &evaluation, &self.provenance) => {
                EvaluationStatus::Complete { evaluation }
            }
            Ok(evaluation) => EvaluationStatus::Incomplete {
                evaluation: Some(evaluation),
            },
            Err(EvaluationFailure::Unavailable) => EvaluationStatus::Unavailable,
            Err(EvaluationFailure::Incomplete) => EvaluationStatus::Incomplete { evaluation: None },
            Err(EvaluationFailure::Failed) => EvaluationStatus::Failed,
            Err(EvaluationFailure::Stopped(reason)) => return Err(reason),
        })
    }
}

impl<E: TacticalEvaluator> GameDetector for HangingMaterialDetector<E> {
    fn descriptor(&self) -> DetectorDescriptor {
        let mut hash = blake3::Hasher::new();
        hash.update(b"tempo-chess/hanging-material/v1");
        hash.update(&self.config.equivalence_cp.to_le_bytes());
        for value in [
            &self.provenance.engine,
            &self.provenance.engine_version,
            &self.provenance.configuration_fingerprint,
        ] {
            hash.update(&(value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
        DetectorDescriptor {
            id: "hanging-material".into(),
            revision: "2".into(),
            configuration_fingerprint: hash.finalize().to_hex().to_string(),
            label: "Hanging material".into(),
        }
    }

    fn detect(
        &self,
        input: &GameDetectionInput,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        let mut findings = Vec::new();
        for occurrence in input.timeline().occurrences() {
            match crate::scan_position(
                input,
                occurrence.offset(),
                [self as &dyn GameDetector],
                control,
            ) {
                DetectionOutcome::Complete(mut moments) => findings.append(&mut moments),
                other => return other,
            }
        }
        DetectionOutcome::Complete(findings)
    }

    fn detect_position(
        &self,
        position: &PositionPass<'_>,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        if let Err(stop) = control.check() {
            return DetectionOutcome::Stopped(stop);
        }
        let input = position.input;
        let ply = position.ply;
        let occurrence = position.occurrence();
        let Some(played) = occurrence
            .next_move()
            .filter(|_| occurrence.side_to_move() == input.learner())
        else {
            return DetectionOutcome::Complete(vec![]);
        };
        let captures = captures_on_board(position.board, &position.legal);
        if captures.is_empty() {
            return DetectionOutcome::Complete(vec![]);
        }
        let request = evaluation_request(input, ply, &position.legal);
        let analysis = match position.compare(&self.provenance, || self.evaluate(&request, control))
        {
            Ok(analysis) => analysis,
            Err(stop) => return DetectionOutcome::Stopped(stop),
        };
        let mut assessment = assess(
            captures,
            played.clone(),
            analysis,
            self.config.equivalence_cp,
        );
        if assessment.outcome == TacticalOutcome::MissedGoodCapture {
            let fen = format!("{} 0 1", occurrence.position().0);
            let standalone = TacticalEvaluationRequest {
                start_fen: fen.clone(),
                fen: fen.clone(),
                moves: Vec::new(),
                legal_moves: request.legal_moves,
            };
            let analysis = match self.evaluate(&standalone, control) {
                Ok(analysis) => analysis,
                Err(stop) => return DetectionOutcome::Stopped(stop),
            };
            finish_standalone(&mut assessment, fen, analysis);
        }
        let mut subjects: Vec<_> = assessment
            .candidates
            .iter()
            .map(|candidate| candidate.capture.target.clone())
            .collect();
        subjects.sort_by(|left, right| left.square.cmp(&right.square));
        subjects.dedup();
        let candidate_move = if assessment.outcome == TacticalOutcome::WinningMate {
            assessment
                .accepted_moves
                .first()
                .map(|answer| answer.uci.clone())
        } else {
            assessment
                .candidates
                .iter()
                .find(|candidate| candidate.quality == CaptureQuality::BestOrEquivalent)
                .or_else(|| assessment.candidates.first())
                .map(|candidate| candidate.capture.answer.uci.clone())
        };
        DetectionOutcome::Complete(vec![DetectedMoment {
            occurrence: ply,
            code: if assessment.outcome == TacticalOutcome::WinningMate {
                "winning_mate".into()
            } else {
                "hanging_material".into()
            },
            subjects,
            candidate_move,
            explanation: explanation(assessment.outcome).into(),
            tactical: Some(assessment),
        }])
    }
}

/// A candidate captures an opposing nonking unit and leaves no legal immediate
/// reply that captures the capturing unit. Full-engine quality is a separate fact.
/// Legal move generation handles pins, king safety, checks and x-ray defenders.
pub fn hanging_captures(fen: &str) -> Result<Vec<HangingCapture>, ChessError> {
    let board = parse_position(fen, false)?;
    let legal = legal_moves(&board);
    Ok(captures_on_board(&board, &legal))
}

fn captures_on_board(board: &Board, legal: &[Move]) -> Vec<HangingCapture> {
    let mut captures = Vec::new();
    for &mv in legal {
        let Some((square, role)) = capture_target(board, mv) else {
            continue;
        };
        let mut after = board.clone();
        after.play_unchecked(mv);
        let mut recaptured = false;
        after.generate_moves(|replies| {
            recaptured = replies.into_iter().any(|reply| reply.to == mv.to);
            recaptured
        });
        if !recaptured {
            captures.push(HangingCapture {
                answer: ChessAnswer {
                    uci: display_uci_move(board, mv).to_string(),
                },
                target: BoardSubject {
                    square: square.to_string(),
                    role,
                    color: LearnerColor::from_board(!board.side_to_move()),
                },
            });
        }
    }
    captures.sort_by(|a, b| a.answer.uci.cmp(&b.answer.uci));
    captures
}

fn capture_target(board: &Board, mv: Move) -> Option<(Square, PieceRole)> {
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
    Some((square, role))
}

fn evaluation_request(
    input: &GameDetectionInput,
    offset: usize,
    legal: &[Move],
) -> TacticalEvaluationRequest {
    let timeline = input.timeline().occurrences();
    let anchor = &timeline[offset];
    TacticalEvaluationRequest {
        start_fen: timeline[0].fen().into(),
        moves: timeline[..offset]
            .iter()
            .map(|position| {
                position
                    .next_move()
                    .expect("nonterminal prefix")
                    .uci
                    .clone()
            })
            .collect(),
        fen: anchor.fen().into(),
        legal_moves: legal
            .iter()
            .map(|&mv| display_uci_move(&anchor.board, mv).to_string())
            .collect(),
    }
}

fn legal_moves(board: &Board) -> Vec<Move> {
    let mut legal = Vec::new();
    board.generate_moves(|moves| {
        legal.extend(moves);
        false
    });
    legal
}

fn complete_evaluation(
    request: &TacticalEvaluationRequest,
    evaluation: &RootEvaluation,
    provenance: &EvaluationProvenance,
) -> bool {
    evaluation.provenance == *provenance
        && [
            &provenance.engine,
            &provenance.engine_version,
            &provenance.configuration_fingerprint,
        ]
        .iter()
        .all(|s| !s.trim().is_empty())
        && evaluation.depth > 0
        && evaluation.nodes > 0
        && evaluation.roots.len() == request.legal_moves.len()
        && evaluation
            .roots
            .iter()
            .map(|r| &r.uci)
            .collect::<BTreeSet<_>>()
            == request.legal_moves.iter().collect::<BTreeSet<_>>()
        && evaluation
            .roots
            .iter()
            .all(|r| r.bound == ScoreBound::Exact && !matches!(r.score, EvaluationScore::Mate(0)))
}

fn score_order(left: EvaluationScore, right: EvaluationScore) -> Ordering {
    use EvaluationScore::*;
    match (left, right) {
        (Centipawns(a), Centipawns(b)) => a.cmp(&b),
        (Mate(a), Mate(b)) if a.signum() == b.signum() => b.cmp(&a),
        (Mate(a), Mate(b)) => a.cmp(&b),
        (Mate(a), Centipawns(_)) => a.cmp(&0),
        (Centipawns(_), Mate(b)) => 0.cmp(&b),
    }
}

fn accepted_roots(status: &EvaluationStatus, tolerance: i32) -> Vec<ChessAnswer> {
    let EvaluationStatus::Complete { evaluation } = status else {
        return Vec::new();
    };
    let Some(best) = evaluation
        .roots
        .iter()
        .map(|r| r.score)
        .max_by(|a, b| score_order(*a, *b))
    else {
        return Vec::new();
    };
    let mut accepted = evaluation
        .roots
        .iter()
        .filter(|root| match (best, root.score) {
            (EvaluationScore::Centipawns(best), EvaluationScore::Centipawns(score)) => {
                i64::from(best) - i64::from(score) <= i64::from(tolerance)
            }
            (EvaluationScore::Mate(best), EvaluationScore::Mate(score)) => best == score,
            _ => false,
        })
        .map(|r| ChessAnswer { uci: r.uci.clone() })
        .collect::<Vec<_>>();
    accepted.sort_by(|a, b| a.uci.cmp(&b.uci));
    accepted
}

fn assess(
    captures: Vec<HangingCapture>,
    played: ChessAnswer,
    analysis: EvaluationStatus,
    equivalence_cp: i32,
) -> TacticalAssessment {
    let accepted_moves = accepted_roots(&analysis, equivalence_cp);
    let scores = match &analysis {
        EvaluationStatus::Complete { evaluation } => evaluation
            .roots
            .iter()
            .map(|r| (&r.uci, r.score))
            .collect::<BTreeMap<_, _>>(),
        _ => BTreeMap::new(),
    };
    if accepted_moves.iter().any(|answer| matches!(scores.get(&answer.uci), Some(EvaluationScore::Mate(distance)) if *distance > 0)) {
        return TacticalAssessment {
            kind: TacticalKind::WinningMate,
            candidates: vec![],
            played,
            outcome: TacticalOutcome::WinningMate,
            analysis,
            standalone_analysis: None,
            accepted_moves,
            puzzle: None,
            equivalence_cp,
        };
    }
    let candidates = captures
        .into_iter()
        .map(|capture| {
            let score = scores.get(&capture.answer.uci).copied();
            let quality = if accepted_moves.is_empty() {
                CaptureQuality::Uncertain
            } else if accepted_moves.contains(&capture.answer) {
                CaptureQuality::BestOrEquivalent
            } else {
                CaptureQuality::Inferior
            };
            HangingCaptureAssessment {
                capture,
                quality,
                score,
            }
        })
        .collect::<Vec<_>>();
    let has_good = candidates
        .iter()
        .any(|c| c.quality == CaptureQuality::BestOrEquivalent);
    let taken = candidates.iter().find(|c| c.capture.answer == played);
    let outcome = if accepted_moves.is_empty() {
        TacticalOutcome::Uncertain
    } else if taken.is_some_and(|c| c.quality == CaptureQuality::BestOrEquivalent) {
        TacticalOutcome::TookGoodCapture
    } else if has_good && accepted_moves.contains(&played) {
        TacticalOutcome::GoodAlternative
    } else if has_good {
        TacticalOutcome::MissedGoodCapture
    } else if taken.is_some() {
        TacticalOutcome::TookInferiorCapture
    } else {
        TacticalOutcome::DeclinedInferiorCapture
    };
    TacticalAssessment {
        kind: TacticalKind::HangingMaterial,
        candidates,
        played,
        outcome,
        analysis,
        standalone_analysis: None,
        accepted_moves,
        puzzle: None,
        equivalence_cp,
    }
}

fn finish_standalone(assessment: &mut TacticalAssessment, fen: String, analysis: EvaluationStatus) {
    let accepted = accepted_roots(&analysis, assessment.equivalence_cp);
    // One Stockfish-ranked capture is the answer, as requested. A score tie is
    // settled by UCI for reproducibility, rather than creating another grader.
    let selected = assessment
        .candidates
        .iter()
        .filter(|c| c.quality == CaptureQuality::BestOrEquivalent)
        .max_by(|a, b| {
            score_order(
                a.score.expect("complete capture score"),
                b.score.expect("complete capture score"),
            )
            .then_with(|| b.capture.answer.uci.cmp(&a.capture.answer.uci))
        })
        .map(|c| &c.capture.answer);
    if let Some(answer) = selected.filter(|answer| accepted.contains(answer)) {
        assessment.puzzle = Some(TacticalPuzzleSolution {
            fen,
            answer: answer.clone(),
        });
    } else {
        assessment.outcome = TacticalOutcome::Uncertain;
    }
    assessment.standalone_analysis = Some(analysis);
}

/// Re-establish all typed classifications from the exact accepted occurrence and
/// supplied engine facts. Board validation alone cannot authorize a puzzle.
pub(crate) fn validate_tactical_assessment(
    input: &GameDetectionInput,
    occurrence: usize,
    assessment: &TacticalAssessment,
) -> bool {
    let Some(anchor) = input.timeline().occurrences().get(occurrence) else {
        return false;
    };
    if anchor.side_to_move() != input.learner()
        || anchor.next_move() != Some(&assessment.played)
        || assessment.equivalence_cp < 0
    {
        return false;
    }
    let legal = legal_moves(&anchor.board);
    let captures = captures_on_board(&anchor.board, &legal);
    if captures.is_empty() {
        return false;
    }
    let request = evaluation_request(input, occurrence, &legal);
    if let EvaluationStatus::Complete { evaluation } = &assessment.analysis {
        if !complete_evaluation(&request, evaluation, &evaluation.provenance) {
            return false;
        }
    }
    let mut expected = assess(
        captures,
        assessment.played.clone(),
        assessment.analysis.clone(),
        assessment.equivalence_cp,
    );
    if expected.outcome == TacticalOutcome::MissedGoodCapture {
        let Some(standalone_analysis) = &assessment.standalone_analysis else {
            return false;
        };
        let fen = format!("{} 0 1", anchor.position().0);
        if let EvaluationStatus::Complete { evaluation } = standalone_analysis {
            let provenance = match &assessment.analysis {
                EvaluationStatus::Complete { evaluation } => &evaluation.provenance,
                _ => return false,
            };
            let standalone = TacticalEvaluationRequest {
                start_fen: fen.clone(),
                fen: fen.clone(),
                moves: vec![],
                legal_moves: request.legal_moves,
            };
            if !complete_evaluation(&standalone, evaluation, provenance) {
                return false;
            }
        }
        finish_standalone(&mut expected, fen, standalone_analysis.clone());
    }
    *assessment == expected
}

fn explanation(outcome: TacticalOutcome) -> &'static str {
    match outcome {
        TacticalOutcome::WinningMate => {
            "Stockfish found a forced mate. Other analyses were skipped."
        }
        TacticalOutcome::TookGoodCapture => {
            "You took a hanging unit with a best or equivalent move."
        }
        TacticalOutcome::GoodAlternative => {
            "A sound capture was available; your alternative was equally good."
        }
        TacticalOutcome::MissedGoodCapture => {
            "A best or equivalent capture was available and the played move was worse."
        }
        TacticalOutcome::TookInferiorCapture => {
            "You took the apparently free material, but another move was better."
        }
        TacticalOutcome::DeclinedInferiorCapture => {
            "Apparently free material was available, but taking it was not best."
        }
        TacticalOutcome::Uncertain => {
            "A capture candidate was found, but its quality is not fully established."
        }
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;

    #[test]
    fn capture_and_mate_checks_share_legal_moves_and_engine_evidence() {
        let played = crate::parse_played_games(
            "[SetUp \"1\"]\n[FEN \"7k/p4Q2/6K1/8/8/8/8/R7 w - - 0 1\"]\n\n1. Qf8# 1-0",
        )
        .unwrap()
        .pop()
        .unwrap()
        .unwrap();
        let input = GameDetectionInput::new(played.timeline(), LearnerColor::White);
        let position = PositionPass::new(&input, 0).unwrap();
        let captures = captures_on_board(position.board, &position.legal);
        assert!(captures.iter().any(|capture| capture.answer.uci == "a1a7"));
        let mating_moves: Vec<_> = position
            .legal
            .iter()
            .filter(|&&mv| {
                let mut after = position.board.clone();
                after.play_unchecked(mv);
                after.status() == cozy_chess::GameStatus::Won
            })
            .collect();
        assert!(!mating_moves.is_empty());
        let profile = EvaluationProvenance {
            engine: "Fixture".into(),
            engine_version: "1".into(),
            configuration_fingerprint: "same".into(),
        };
        let score = position.compare(&profile, || {
            Ok(EvaluationStatus::Incomplete { evaluation: None })
        });
        assert_eq!(
            score,
            position.compare(&profile, || panic!("duplicate evaluation"))
        );
    }
}
