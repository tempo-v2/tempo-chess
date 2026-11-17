//! PositionKey and its usage scenarios.

use serde::{Deserialize, Serialize};

/// A canonical position identity, distinct from the FEN used to draw a board.
///
/// # When to use
///
/// Use this key to recognize a repertoire position reached through different move
/// orders. A [`crate::PositionCanonicalizer`] produces it; [`crate::ContentKind`]
/// and catalog validation consume it. The repertoire name is also part of a
/// repertoire target's identity, so the same position may belong to two repertoires.
///
/// The wrapper does not canonicalize or validate its string. Production
/// canonicalization must account for side to move, castling and legally effective
/// en-passant rights while ignoring move counters. Current fixtures register the
/// mapping explicitly; they do not implement those chess rules.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PositionKey(pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentKind, PositionCanonicalizer, START_FEN, demo_canonicalizer, demo_catalog};

    #[test]
    fn a_board_key_resolves_a_position_within_its_repertoire() {
        let key = demo_canonicalizer().canonicalize(START_FEN).unwrap();
        let catalog = demo_catalog();
        let matching = catalog
            .items
            .values()
            .filter(|item| {
                matches!(&item.definition.kind,
            ContentKind::Repertoire { repertoire, position }
            if repertoire == "r1" && position == &key)
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].target.0, "r1:p1");
        assert_eq!(key, PositionKey("start".into()));
    }
}
