//! FixtureCanonicalizer and its usage scenarios.

use crate::{ChessError, PositionCanonicalizer, PositionKey};
use std::collections::BTreeMap;

/// An explicit FEN-to-key lookup for demos and interface tests.
///
/// # When to use
///
/// Register known boards with `with_position` when testing an importer without a
/// real chess library. Different registered FENs may map to the same key, making
/// an identity-sharing scenario explicit.
///
/// This type performs no normalization or legality calculation. Unknown FENs fail
/// rather than receiving a guessed position identity.
#[derive(Clone, Debug, Default)]
pub struct FixtureCanonicalizer {
    positions: BTreeMap<String, PositionKey>,
}
impl FixtureCanonicalizer {
    pub fn with_position(mut self, fen: impl Into<String>, key: PositionKey) -> Self {
        self.positions.insert(fen.into(), key);
        self
    }
}
impl PositionCanonicalizer for FixtureCanonicalizer {
    fn canonicalize(&self, fen: &str) -> Result<PositionKey, ChessError> {
        self.positions.get(fen).cloned().ok_or_else(|| {
            ChessError::UnknownFixture("FEN is not registered in the fixture canonicalizer".into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::START_FEN;

    #[test]
    fn explicit_counter_variants_can_share_a_registered_identity() {
        let with_later_counters = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 4 3";
        let key = PositionKey("start".into());
        let canonicalizer = FixtureCanonicalizer::default()
            .with_position(START_FEN, key.clone())
            .with_position(with_later_counters, key);
        let first = canonicalizer.canonicalize(START_FEN).unwrap();
        let repeated = canonicalizer.canonicalize(with_later_counters).unwrap();
        assert_eq!(first, repeated);
        assert!(canonicalizer.canonicalize("not registered").is_err());
    }
}
