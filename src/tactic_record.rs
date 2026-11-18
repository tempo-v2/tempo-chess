//! A tactic ready for compilation from its learner's turn.

use std::fmt::Write;
use tempo_types::TargetId;

/// Use after source conversion. `solution` contains every UCI ply, starting with
/// the learner. A trailing opponent reply can remain in the solution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TacticRecord {
    pub namespace: String,
    pub source_id: String,
    pub solver_fen: String,
    pub solution: Vec<String>,
    pub label: String,
    pub provenance: Vec<String>,
}

impl TacticRecord {
    /// Stable ASCII encoding of the source pair. Each component is lowercase
    /// hexadecimal UTF-8 and can include any Unicode or delimiters in the source.
    /// The compiler records this identifier in provenance, including for aliases.
    pub fn target_id(&self) -> TargetId {
        let mut key = String::from("tactic:");
        for byte in self.namespace.bytes() {
            write!(key, "{byte:02x}").expect("writing to String is infallible");
        }
        key.push(':');
        for byte in self.source_id.bytes() {
            write!(key, "{byte:02x}").expect("writing to String is infallible");
        }
        TargetId(key)
    }
}

pub(crate) fn is_source_token(value: &str) -> bool {
    fn hex_component(value: &str) -> bool {
        !value.is_empty()
            && value.len() % 2 == 0
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
    value
        .strip_prefix("tactic:")
        .and_then(|pair| pair.split_once(':'))
        .is_some_and(|(namespace, id)| hex_component(namespace) && hex_component(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::START_FEN;

    #[test]
    fn source_identity_uses_an_unambiguous_pair_and_survives_content_edits() {
        let mut record = TacticRecord {
            namespace: "a:b".into(),
            source_id: "é".into(),
            solver_fen: START_FEN.into(),
            solution: vec!["e2e4".into()],
            label: "First".into(),
            provenance: vec![],
        };
        let original = record.target_id();
        assert_eq!(original.0, "tactic:613a62:c3a9");
        record.solution[0] = "d2d4".into();
        record.label = "Edited".into();
        assert_eq!(record.target_id(), original);
        record.namespace = "a".into();
        record.source_id = "b:é".into();
        assert_ne!(record.target_id(), original);
    }
}
