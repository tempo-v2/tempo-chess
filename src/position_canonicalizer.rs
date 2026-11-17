//! PositionCanonicalizer and its usage scenarios.

use crate::{ChessError, PositionKey};

/// The boundary between a board FEN and stable position identity.
///
/// # When to use
///
/// A chess importer calls this while assigning repertoire targets and resolving
/// transpositions. The result is a [`PositionKey`], not a rewritten display FEN.
/// One consistent implementation must define identity for every import path.
///
/// The intended production implementation uses chess rules to handle effective
/// en-passant rights and castling while excluding move counters. Current
/// [`crate::FixtureCanonicalizer`] entries only demonstrate the interface.
pub trait PositionCanonicalizer {
    fn canonicalize(&self, fen: &str) -> Result<PositionKey, ChessError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{START_FEN, demo_canonicalizer};

    #[test]
    fn import_code_requests_position_identity_through_the_trait() {
        let canonicalizer: &dyn PositionCanonicalizer = &demo_canonicalizer();
        assert_eq!(
            canonicalizer.canonicalize(START_FEN).unwrap(),
            PositionKey("start".into())
        );
        assert!(matches!(
            canonicalizer.canonicalize("unknown FEN"),
            Err(ChessError::UnknownFixture(_))
        ));
    }
}
