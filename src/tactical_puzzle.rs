//! Compile generated single-answer tactics through the existing content path.

use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::{
    Catalog, ChangeKind, ChessAnswer, ChessError, ContentPlan, ContentStatus, TacticRecord,
    compile_tactics, plan_tactic_capture,
};
use tempo_types::TargetId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TacticalPuzzlePlan {
    pub content: ContentPlan,
    pub target: TargetId,
}

/// Plan the one engine-selected capture using the existing exact-sequence
/// compiler and canonical board/solution deduplication. The caller has already
/// validated this selected answer. An exact match quarantined by an unresolved
/// import conflict becomes available again as a metadata change; its identity,
/// definition and version are retained, as are all other conflicting targets.
/// This function changes no learning.
pub fn plan_tactical_puzzle(
    current: &Catalog,
    fen: &str,
    selected: &ChessAnswer,
    provenance: &[String],
) -> Result<TacticalPuzzlePlan, ChessError> {
    let board = parse_position(fen, false)?;
    let position = board_key(&board);
    let mut hash = blake3::Hasher::new();
    hash.update(b"tempo-chess/generated-capture/v1");
    for value in [position.0.as_str(), selected.uci.as_str()] {
        hash.update(&(value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    let record = TacticRecord {
        namespace: "tempo-generated".into(),
        source_id: hash.finalize().to_hex().to_string(),
        solver_fen: format!("{} 0 1", position.0),
        solution: vec![selected.uci.clone()],
        label: "From your games".into(),
        provenance: provenance.to_vec(),
    };
    let incoming = compile_tactics(&[record])?;
    let mut plan = plan_tactic_capture(current, &incoming)?;
    let target = plan.content.changes[0].target.clone();
    let retained = plan
        .content
        .catalog
        .items
        .get_mut(&target)
        .expect("planned tactic");
    if retained.status == ContentStatus::Quarantined {
        retained.status = ContentStatus::Active;
        plan.content.changes[0].kind = ChangeKind::Metadata;
    }
    if !retained.active() {
        return Err(ChessError::InvalidContent(
            "the equivalent tactical target is inactive".into(),
        ));
    }
    plan.content.catalog.validate()?;
    Ok(TacticalPuzzlePlan {
        content: plan.content,
        target,
    })
}
