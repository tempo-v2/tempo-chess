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
    /// Admit only a line with a demonstrated motif. Compute the original
    /// generated identity from the selected root before replacing the line.
    pub fn qualify(mut self, variation: Option<&[String]>) -> Result<Option<Self>, ChessError> {
        if !self.single_root {
            let first = &self.item.definition.decisions[0];
            let line: Vec<_> = definition_line(&self.item)
                .into_iter()
                .map(|m| m.uci)
                .collect();
            if let Some(proof) = crate::recognizable_payoff(&first.fen, &first.expected, &line)? {
                self.item.verified_tactic = proof.motifs;
            }
            return Ok(Some(self));
        }
        let decision = &self.item.definition.decisions[0];
        let first = [decision.expected.uci.clone()];
        let Some(proof) = crate::recognizable_payoff(
            &decision.fen,
            &decision.expected,
            variation.unwrap_or(&first),
        )?
        else {
            return Ok(None);
        };
        self.item.definition = proved_definition(&decision.fen, &proof)?;
        if crate::ambiguous_hanging_material(&self.item.definition, &proof.motifs)? {
            return Ok(None);
        }
        self.item.verified_tactic = proof.motifs;
        Ok(Some(self))
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
            self.item.verified_tactic.clone_from(&prior.verified_tactic);
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
        if line.len() == 3 && !crate::personal_tactics::mate_moves(&board).is_empty() {
            return Err(ChessError::InvalidContent(
                "the stored mating finish would be ambiguous with an earlier mate".into(),
            ));
        }
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

/// Classify only the original output of this compiler, after Store has
/// established the game binding. Authored and imported aliases own their
/// definitions independently, even when a game finding refers to them.
pub fn game_generated_definition(item: &ContentItem) -> bool {
    if !item.active()
        || item.game_capture.is_some()
        || item
            .provenance
            .iter()
            .any(|key| crate::tactic_record::is_source_token(key) && *key != item.target.0)
    {
        return false;
    }
    let Some(first) = item.definition.decisions.first() else {
        return false;
    };
    let root = prepare_tactical_puzzle(&first.fen, &first.expected, &[]).ok();
    root.is_some_and(|root| root.item.target == item.target)
        || (item.definition.scheme == crate::GradingScheme::ForcedMateV1
            && prepare_line(&first.fen, &definition_line(item), true, &[])
                .is_ok_and(|mate| mate.item.target == item.target))
}

fn definition_line(item: &ContentItem) -> Vec<ChessAnswer> {
    item.definition
        .decisions
        .iter()
        .flat_map(|d| std::iter::once(d.expected.clone()).chain(d.autoplay.iter().cloned()))
        .collect()
}

fn proved_definition(
    fen: &str,
    proof: &crate::TacticalPayoff,
) -> Result<crate::ChessDefinition, ChessError> {
    let mut definition = prepare_line(fen, &proof.line, false, &[])?.item.definition;
    if proof
        .motifs
        .last()
        .is_some_and(|m| matches!(m.motif, crate::Motif::MateInOne | crate::Motif::MateInTwo))
    {
        // The exact-sequence grader plays the prefix, then accepts any
        // legal mate on the final decision.
        definition.scheme = crate::GradingScheme::ForcedMateV1;
    }
    definition.validate()?;
    Ok(definition)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameTacticQualification {
    Kept,
    Revised(Box<GeneratedTacticExtension>),
    Withheld,
}

/// Verify an existing generated card against a contemporary defense search.
/// Existing full short mates also have a complete, independently checkable
/// line and can be evaluated with their issued replies.
pub fn requalify_game_tactic(
    item: &ContentItem,
    variation: &[String],
) -> Result<GameTacticQualification, ChessError> {
    if !game_generated_definition(item) {
        return Err(ChessError::InvalidContent(
            "expected game-generated content".into(),
        ));
    }
    let first = &item.definition.decisions[0];
    let retained: Vec<_> = definition_line(item).into_iter().map(|m| m.uci).collect();
    let line = if item.definition.scheme == crate::GradingScheme::ForcedMateV1
        && prepare_line(&first.fen, &definition_line(item), true, &[])
            .is_ok_and(|mate| mate.item.target == item.target)
    {
        &retained
    } else {
        variation
    };
    let Some(proof) = crate::recognizable_payoff(&first.fen, &first.expected, line)? else {
        return Ok(GameTacticQualification::Withheld);
    };
    let definition = proved_definition(&first.fen, &proof)?;
    if crate::ambiguous_hanging_material(&definition, &proof.motifs)? {
        return Ok(GameTacticQualification::Withheld);
    }
    if definition == item.definition && proof.motifs == item.verified_tactic {
        return Ok(GameTacticQualification::Kept);
    }
    let mut next = item.clone();
    next.definition = definition;
    next.verified_tactic = proof.motifs;
    next.payoff_origin_version = Some(item.payoff_origin_version.unwrap_or(item.version));
    next.version.0 = item
        .version
        .0
        .checked_add(1)
        .ok_or_else(|| ChessError::InvalidContent("content version exhausted".into()))?;
    Ok(GameTacticQualification::Revised(Box::new(
        GeneratedTacticExtension {
            prior: item.clone(),
            next,
        },
    )))
}

/// A verified replacement and the exact current item from which it was derived.
#[derive(Clone, Debug, PartialEq, Eq)]
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
