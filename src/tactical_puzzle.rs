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
pub struct GeneratedTacticalPuzzle {
    item: ContentItem,
    single_root: bool,
}

impl GeneratedTacticalPuzzle {
    /// Extend a newly proposed root only when its own engine line settles the tactic.
    pub fn with_payoff(mut self, variation: &[String]) -> Self {
        // Optional analysis cannot invalidate an already accepted puzzle.
        if let Ok(Some(extended)) = extend_generated_tactic(&self.item, variation) {
            self.item.definition = extended.next.definition;
        }
        self
    }

    /// Fetch current items matching any of these keys before calling `plan`.
    pub fn lookup(&self) -> TacticLookup {
        TacticLookup::from_item(&self.item).expect("generated tactic")
    }

    /// `current` may hold only matches by target, exact identity and source
    /// token. Chess still verifies compatibility and owns provenance merging.
    pub fn plan(mut self, current: &Catalog) -> Result<TacticalPuzzlePlan, ChessError> {
        // Game generation binds a position and its selected root. A later
        // engine PV may vary; only the versioned upgrade changes a retained line.
        if self.single_root
            && let Some(prior) = current.items.values().find(|item| {
                item.target == self.item.target || item.provenance.contains(&self.item.target.0)
            })
            && prior.definition.kind == self.item.definition.kind
            && prior.definition.decisions.first().map(|d| &d.expected)
                == self.item.definition.decisions.first().map(|d| &d.expected)
        {
            self.item.definition.clone_from(&prior.definition);
        }
        let mut plan = crate::plan_tactic_capture::plan_tactic_items(current, [&self.item])?;
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
    Ok(GeneratedTacticalPuzzle {
        item,
        single_root: !mate && line.len() == 1,
    })
}

/// Eligible legacy shape after Store establishes a game training binding.
/// Recompute the original generated key and exclude items shared with authors.
pub fn short_generated_decision(item: &ContentItem) -> Option<&crate::Decision> {
    let [decision] = item.definition.decisions.as_slice() else {
        return None;
    };
    if !item.active()
        || item.game_capture.is_some()
        || !decision.autoplay.is_empty()
        || item.definition.scheme != crate::GradingScheme::ExactSequenceV1
        || item
            .provenance
            .iter()
            .any(|p| crate::tactic_record::is_source_token(p) && *p != item.target.0)
    {
        return None;
    }
    let generated = prepare_tactical_puzzle(&decision.fen, &decision.expected, &[]).ok()?;
    (item.target == generated.item.target && item.definition == generated.item.definition)
        .then_some(decision)
}

/// A verified replacement and the exact current item from which it was derived.
pub struct GeneratedTacticExtension {
    prior: ContentItem,
    next: ContentItem,
}
impl GeneratedTacticExtension {
    pub fn prior(&self) -> &ContentItem {
        &self.prior
    }
    pub fn replacement(&self) -> &ContentItem {
        &self.next
    }
}

/// Produce the next definition for the original generated target. Store
/// verifies the old item and version again when it publishes the replacement.
pub fn extend_generated_tactic(
    item: &ContentItem,
    variation: &[String],
) -> Result<Option<GeneratedTacticExtension>, ChessError> {
    let Some(d) = short_generated_decision(item) else {
        return Ok(None);
    };
    let Some(line) = crate::tactical_payoff(&d.fen, &d.expected, variation)? else {
        return Ok(None);
    };
    if line.len() == 1 {
        return Ok(None);
    }
    let mut next = item.clone();
    next.payoff_origin_version = Some(item.version);
    next.definition = prepare_line(&d.fen, &line, false, &[])?.item.definition;
    let last = next
        .definition
        .decisions
        .last()
        .expect("nonempty verified line");
    let mut finish = parse_position(&last.fen, true)?;
    finish.play_unchecked(crate::standard_uci::legal_move(
        &finish,
        &last.expected.uci,
    )?);
    if finish.status() == cozy_chess::GameStatus::Won {
        // This scheme replays an alternate mating finish from the last board.
        // Only use a continuation when earlier decisions cannot already mate.
        for d in &next.definition.decisions[..next.definition.decisions.len() - 1] {
            let board = parse_position(&d.fen, true)?;
            if board.generate_moves(|moves| {
                moves.into_iter().any(|mv| {
                    let mut after = board.clone();
                    after.play_unchecked(mv);
                    after.status() == cozy_chess::GameStatus::Won
                })
            }) {
                return Ok(None);
            }
        }
        next.definition.scheme = crate::GradingScheme::ForcedMateV1;
    }
    next.version.0 = next
        .version
        .0
        .checked_add(1)
        .ok_or_else(|| ChessError::InvalidContent("content version exhausted".into()))?;
    Ok(Some(GeneratedTacticExtension {
        prior: item.clone(),
        next,
    }))
}
