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
    plan_line(
        current,
        fen,
        std::slice::from_ref(selected),
        false,
        provenance,
    )
}

pub fn plan_personal_puzzle(
    current: &Catalog,
    puzzle: &crate::PersonalPuzzle,
    provenance: &[String],
) -> Result<TacticalPuzzlePlan, ChessError> {
    plan_line(
        current,
        &puzzle.fen,
        &puzzle.line,
        puzzle.kind == crate::PersonalPuzzleKind::ForcedMate,
        provenance,
    )
}

fn plan_line(
    current: &Catalog,
    fen: &str,
    line: &[ChessAnswer],
    mate: bool,
    provenance: &[String],
) -> Result<TacticalPuzzlePlan, ChessError> {
    let board = parse_position(fen, false)?;
    let position = board_key(&board);
    let mut hash = blake3::Hasher::new();
    hash.update(b"tempo-chess/generated-capture/v1");
    if mate {
        hash.update(b"forced-mate");
    }
    for value in
        std::iter::once(position.0.as_str()).chain(line.iter().map(|answer| answer.uci.as_str()))
    {
        hash.update(&(value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    let record = TacticRecord {
        namespace: "tempo-generated".into(),
        source_id: hash.finalize().to_hex().to_string(),
        solver_fen: format!("{} 0 1", position.0),
        solution: line.iter().map(|answer| answer.uci.clone()).collect(),
        label: "From your games".into(),
        provenance: provenance.to_vec(),
    };
    let mut incoming = compile_tactics(&[record])?;
    if mate {
        let mut finish = board.clone();
        for answer in line {
            let mv = crate::standard_uci::legal_move(&finish, &answer.uci)?;
            finish.play_unchecked(mv);
        }
        if finish.status() != cozy_chess::GameStatus::Won || !matches!(line.len(), 1 | 3) {
            return Err(ChessError::InvalidContent(
                "forced mate needs a legal mating line of one or three plies".into(),
            ));
        }
        incoming
            .items
            .values_mut()
            .next()
            .expect("one generated tactic")
            .definition
            .scheme = crate::GradingScheme::ForcedMateV1;
    }
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
