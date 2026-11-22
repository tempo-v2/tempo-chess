//! Typed tactic membership and the explicit legacy ingress codec.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const LEGACY_PACK_PREFIX: &str = "tempo1:catalog-metadata:";

/// Validated theme membership for new tactic admission.
///
/// An empty set is known to have no recognized membership. A persistence owner
/// can distinguish an unmigrated record by wrapping these facts in an Option.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TacticPolicyFacts {
    theme_ids: BTreeSet<String>,
}

impl TacticPolicyFacts {
    pub fn theme_ids(&self) -> &BTreeSet<String> {
        &self.theme_ids
    }

    /// Normalize the historical transfer/catalog encoding at import or migration.
    /// The caller supplies the authoritative vocabulary used by its theme settings.
    /// Once normalized, read the typed facts without interpreting display fields.
    pub fn from_legacy_metadata<'a>(
        entries: impl IntoIterator<Item = &'a str>,
        is_known_theme: impl Fn(&str) -> bool,
    ) -> Self {
        let theme_ids = entries
            .into_iter()
            .filter_map(legacy_theme)
            .filter(|theme| is_known_theme(theme))
            .map(str::to_owned)
            .collect();
        Self { theme_ids }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LegacyMember<'a> {
    motif: &'a str,
    difficulty: &'a str,
    deck_id: &'a str,
    deck_position: u64,
}

fn legacy_theme(entry: &str) -> Option<&str> {
    let member: LegacyMember<'_> =
        serde_json::from_str(entry.strip_prefix(LEGACY_PACK_PREFIX)?).ok()?;
    // Difficulty and deck order validate the legacy format only.
    if member.deck_position == 0
        || !matches!(
            member.difficulty,
            "easy" | "medium" | "hard" | "advanced" | "focused"
        )
    {
        return None;
    }
    let (pack, ordinal) = member.deck_id.rsplit_once('-')?;
    let (theme, difficulty) = pack.rsplit_once('-')?;
    let pack_ordinal: u64 = ordinal.parse().ok()?;
    (theme == member.motif
        && difficulty == member.difficulty
        && pack_ordinal > 0
        && ordinal == format!("{pack_ordinal:02}"))
    .then_some(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tactic_metadata_normalizes_archived_memberships_to_deduplicated_themes() {
        let entries = [
            r#"tempo1:catalog-metadata:{"DeckId": "fork-focused-04", "DeckPosition": 9, "Difficulty": "focused", "GameUrl": "https://example.invalid/game", "Motif": "fork"}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"hard","DeckId":"fork-hard-01","DeckPosition":2}"#,
            r#"tempo1:catalog-metadata:{"Motif":"pin","Difficulty":"advanced","DeckId":"pin-advanced-03","DeckPosition":1}"#,
        ];
        let facts = TacticPolicyFacts::from_legacy_metadata(entries, |theme| {
            matches!(theme, "fork" | "pin")
        });
        assert_eq!(
            facts.theme_ids(),
            &BTreeSet::from(["fork".into(), "pin".into()])
        );
        let reformatted = [entries[2], "unrelated attribution", entries[0], entries[1]];
        assert_eq!(
            TacticPolicyFacts::from_legacy_metadata(reformatted, |theme| {
                matches!(theme, "fork" | "pin")
            }),
            facts,
        );
    }

    #[test]
    fn tactic_metadata_rejects_invalid_and_unknown_membership() {
        let entries = [
            "tempo1:legacy-deck:fork-easy-01",
            "tempo1:catalog-metadata:{",
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"easy","DeckId":"pin-easy-01","DeckPosition":1}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"hard","DeckId":"fork-easy-01","DeckPosition":1}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"easy","DeckId":"fork-easy-00","DeckPosition":1}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"easy","DeckId":"fork-easy-+1","DeckPosition":1}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"easy","DeckId":"fork-easy-01","DeckPosition":0}"#,
            r#"tempo1:catalog-metadata:{"Motif":"fork","Difficulty":"expert","DeckId":"fork-expert-01","DeckPosition":1}"#,
            r#"tempo1:catalog-metadata:{"Motif":"unknown","Difficulty":"easy","DeckId":"unknown-easy-01","DeckPosition":1}"#,
        ];
        for entry in entries {
            assert_eq!(
                TacticPolicyFacts::from_legacy_metadata([entry], |theme| theme == "fork"),
                TacticPolicyFacts::default(),
                "{entry}",
            );
        }
    }

    #[test]
    fn tactic_metadata_explicit_empty_round_trips() {
        let facts = Some(TacticPolicyFacts::default());
        let encoded = serde_json::to_string(&facts).unwrap();
        assert_eq!(encoded, r#"{"theme_ids":[]}"#);
        assert_eq!(
            serde_json::from_str::<Option<TacticPolicyFacts>>(&encoded).unwrap(),
            facts
        );
        assert_eq!(
            serde_json::from_str::<Option<TacticPolicyFacts>>("null").unwrap(),
            None
        );
    }
}
