//! Compile generated single-answer tactics through the existing content path.

use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::{
    Catalog, ChangeKind, ChessAnswer, ChessError, ContentItem, ContentPlan, ContentStatus,
    TacticLookup, TacticRecord, compile_tactics,
};
use tempo_types::TargetId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TacticalPuzzlePlan {
    pub content: ContentPlan,
    pub target: TargetId,
}

/// One legal generated tactic before comparison with current content.
pub struct GeneratedTacticalPuzzle(ContentItem);

impl GeneratedTacticalPuzzle {
    /// Fetch current items matching any of these keys before calling `plan`.
    pub fn lookup(&self) -> TacticLookup {
        TacticLookup::from_item(&self.0).expect("generated tactic")
    }

    /// `current` may hold only matches by target, exact identity and source
    /// token. Chess still verifies compatibility and owns provenance merging.
    pub fn plan(self, current: &Catalog) -> Result<TacticalPuzzlePlan, ChessError> {
        let mut plan = crate::plan_tactic_capture::plan_tactic_items(current, [&self.0])?;
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
    prepare_tactical_puzzle(fen, selected, provenance)?.plan(current)
}

pub fn plan_personal_puzzle(
    current: &Catalog,
    puzzle: &crate::PersonalPuzzle,
    provenance: &[String],
) -> Result<TacticalPuzzlePlan, ChessError> {
    prepare_personal_puzzle(puzzle, provenance)?.plan(current)
}

pub fn prepare_tactical_puzzle(
    fen: &str,
    selected: &ChessAnswer,
    provenance: &[String],
) -> Result<GeneratedTacticalPuzzle, ChessError> {
    prepare_line(fen, std::slice::from_ref(selected), false, provenance)
}

pub fn prepare_personal_puzzle(
    puzzle: &crate::PersonalPuzzle,
    provenance: &[String],
) -> Result<GeneratedTacticalPuzzle, ChessError> {
    prepare_line(
        &puzzle.fen,
        &puzzle.line,
        puzzle.kind == crate::PersonalPuzzleKind::ForcedMate,
        provenance,
    )
}

fn prepare_line(
    fen: &str,
    line: &[ChessAnswer],
    mate: bool,
    provenance: &[String],
) -> Result<GeneratedTacticalPuzzle, ChessError> {
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
    let mut item = compile_tactics(&[record])?
        .items
        .into_values()
        .next()
        .expect("one generated tactic");
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
        item.definition.scheme = crate::GradingScheme::ForcedMateV1;
    }
    item.definition.validate()?;
    Ok(GeneratedTacticalPuzzle(item))
}
