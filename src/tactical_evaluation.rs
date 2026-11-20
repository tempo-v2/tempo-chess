//! Engine evidence supplied by the host, without process or storage dependencies.

use crate::{DetectionControl, DetectionStop};
use serde::{Deserialize, Serialize};

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
