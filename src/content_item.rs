//! ContentItem and its usage scenarios.

use crate::{ChessDefinition, ContentStatus, RepertoireLesson};
use serde::{Deserialize, Serialize};
use tempo_study::Exercise;
use tempo_types::{ContentVersion, TargetId};

/// One versioned learning target, its definition, and its descriptive metadata.
///
/// # When to use
///
/// A parser/import planner creates content items; the application stores them in a
/// [`crate::Catalog`] and resolves selected targets into [`Exercise`] values.
/// Labels and provenance belong here because they are content metadata, not rules
/// the grader needs.
///
/// `target` is stable across edits. `version` changes when the prescribed response
/// or other definition semantics change; a label-only edit preserves it. Old
/// attempts remain tied to the version they were issued. Use
/// [`crate::plan_import`] to make that distinction rather than assigning versions
/// manually.
///
/// `exercise()` copies identity, version and definition only. It does **not** check
/// availability: callers check `active()` before issuing new work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentItem {
    pub target: TargetId,
    pub version: ContentVersion,
    pub label: String,
    pub status: ContentStatus,
    pub definition: ChessDefinition,
    pub provenance: Vec<String>,
    /// Earlier definition whose accepted game finding supplied this payoff extension.
    /// Used only to carry related-game attribution across this exact version change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payoff_origin_version: Option<ContentVersion>,
    /// Verified setup and finishing moves for generated Study clues.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_tactic: Vec<crate::PayoffMotif>,
    /// An archived game capture verified against this exact definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_capture: Option<GameCapture>,
    /// Evidence for the first opponent move, independent of grading.
    #[serde(default, skip_serializing_if = "crate::LeadInEvidence::is_unavailable")]
    pub opponent_lead_in: crate::LeadInEvidence,
    /// PGN context for explicit teaching. Keep it out of blind test views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<RepertoireLesson>,
}

/// An authoritative capture record whose board and line match the tactic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCapture {
    pub capture_id: String,
}

impl ContentItem {
    pub fn exercise(&self) -> Exercise<ChessDefinition> {
        Exercise {
            target: self.target.clone(),
            version: self.version,
            definition: self.definition.clone(),
        }
    }
    /// Check retained metadata against the issued decision before using it.
    pub fn verified_opponent_lead_in(&self) -> Option<crate::OpponentLeadIn> {
        let lead_in = self.opponent_lead_in.verified_move()?;
        let solver = self.definition.decisions.first()?;
        crate::OpponentLeadIn::verified(&lead_in.before_fen, &lead_in.uci, &solver.fen)
    }

    pub fn active(&self) -> bool {
        self.status == ContentStatus::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo_catalog, plan_import};

    #[test]
    fn resolving_content_into_study_keeps_identity_and_the_version_being_answered() {
        let current = demo_catalog();
        let target = TargetId::from("r1:p1");
        let issued_exercise = current.items[&target].exercise();
        let mut incoming = current.clone();
        incoming
            .items
            .get_mut(&target)
            .unwrap()
            .definition
            .decisions[0]
            .expected = "d2d4".into();
        let plan = plan_import(&current, &incoming).unwrap();
        let next_exercise = plan.catalog.items[&target].exercise();
        assert_eq!(issued_exercise.target, next_exercise.target);
        assert_eq!(issued_exercise.version, ContentVersion(1));
        assert_eq!(next_exercise.version, ContentVersion(2));
        assert_eq!(issued_exercise.definition.decisions[0].expected.uci, "e2e4");
        assert_eq!(next_exercise.definition.decisions[0].expected.uci, "d2d4");
    }
}
