//! Engine evidence supplied by the host, without process or storage dependencies.

use crate::{
    ChessAnswer, DetectionControl, DetectionStop, GameDetectionInput, PositionPass,
    TacticalAssessment, TacticalKind, TacticalOutcome,
};
use cozy_chess::{Move, util::display_uci_move};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeSet};

/// Stable evaluation identity. Changing search settings changes the fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationProvenance {
    pub engine: String,
    pub engine_version: String,
    pub configuration_fingerprint: String,
}

/// Replay the accepted prefix before evaluating the exact board. Every score is
/// from this root's side-to-move perspective, not the engine's resulting leaf.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TacticalEvaluationRequest {
    pub start_fen: String,
    pub moves: Vec<String>,
    pub fen: String,
    pub legal_moves: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EvaluationScore {
    Centipawns(i32),
    /// Signed UCI mate distance. Zero is not valid at a nonterminal root.
    Mate(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreBound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootScore {
    pub uci: String,
    pub score: EvaluationScore,
    pub bound: ScoreBound,
}

/// A complete comparable MultiPV iteration. The consumer independently checks
/// coverage, unique legal roots, exact scores and the registered provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootEvaluation {
    pub provenance: EvaluationProvenance,
    pub depth: u32,
    pub nodes: u64,
    pub roots: Vec<RootScore>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationFailure {
    Unavailable,
    Incomplete,
    Failed,
    Stopped(DetectionStop),
}

/// Implemented by the host's bounded engine adapter. Chess provides only values.
pub trait TacticalEvaluator: Send + Sync {
    fn descriptor(&self) -> EvaluationProvenance;
    fn evaluate(
        &self,
        request: &TacticalEvaluationRequest,
        control: &dyn DetectionControl,
    ) -> Result<RootEvaluation, EvaluationFailure>;
}

/// A failed or partial search remains visible and cannot create a training miss.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EvaluationStatus {
    Complete { evaluation: RootEvaluation },
    Unavailable,
    Incomplete { evaluation: Option<RootEvaluation> },
    Failed,
}

pub(crate) fn checked_evaluation(
    evaluator: &dyn TacticalEvaluator,
    request: &TacticalEvaluationRequest,
    control: &dyn DetectionControl,
) -> Result<EvaluationStatus, DetectionStop> {
    control.check()?;
    let result = evaluator.evaluate(request, control);
    control.check()?;
    Ok(match result {
        Ok(evaluation) if complete_evaluation(request, &evaluation, &evaluator.descriptor()) => {
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

pub(crate) fn evaluation_request(
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

pub(crate) fn complete_evaluation(
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

pub(crate) fn score_order(left: EvaluationScore, right: EvaluationScore) -> Ordering {
    use EvaluationScore::*;
    match (left, right) {
        (Centipawns(a), Centipawns(b)) => a.cmp(&b),
        (Mate(a), Mate(b)) if a.signum() == b.signum() => b.cmp(&a),
        (Mate(a), Mate(b)) => a.cmp(&b),
        (Mate(a), Centipawns(_)) => a.cmp(&0),
        (Centipawns(_), Mate(b)) => 0.cmp(&b),
    }
}

pub(crate) fn accepted_roots(status: &EvaluationStatus, tolerance: i32) -> Vec<ChessAnswer> {
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

/// A completed position evaluation can establish mate on any legal board.
pub(crate) fn validate_winning_mate(
    position: &PositionPass<'_>,
    assessment: &TacticalAssessment,
) -> bool {
    let EvaluationStatus::Complete { evaluation } = &assessment.analysis else {
        return false;
    };
    if assessment.kind != TacticalKind::WinningMate
        || assessment.outcome != TacticalOutcome::WinningMate
        || position.occurrence().next_move() != Some(&assessment.played)
        || position.occurrence().side_to_move() != position.learner()
        || !assessment.candidates.is_empty()
        || assessment.standalone_analysis.is_some()
        || assessment.puzzle.is_some()
        || assessment.personal.is_some()
        || !complete_evaluation(
            &position.engine_request(),
            evaluation,
            &evaluation.provenance,
        )
    {
        return false;
    }
    let accepted = accepted_roots(&assessment.analysis, 0);
    !accepted.is_empty()
        && assessment.accepted_moves == accepted
        && accepted.iter().all(|answer| {
            evaluation.roots.iter().any(|root| {
                root.uci == answer.uci
                    && matches!(root.score, EvaluationScore::Mate(distance) if distance > 0)
            })
        })
}
