//! GameComparator and its usage scenarios.

use crate::{Catalog, ChessError, Deviation};

/// A pure comparison of supplied game text with supplied content.
///
/// # When to use
///
/// After provider I/O, the application passes a game and catalog here and receives
/// [`Deviation`] facts. It decides whether those facts should cause external
/// assessments. Study does not fetch games and this interface does not receive a
/// store or modify the catalog.
///
/// [`crate::FixtureComparator`] supports registered games for wiring. A real
/// implementation replaces the comparison algorithm behind this same trait.
pub trait GameComparator {
    fn compare(&self, pgn: &str, catalog: &Catalog) -> Result<Vec<Deviation>, ChessError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GAME_PGN, demo_catalog, demo_comparator};
    use tempo_types::TargetId;

    #[test]
    fn a_background_game_scan_uses_the_trait_with_an_explicit_content_snapshot() {
        let comparator: &dyn GameComparator = &demo_comparator();
        let catalog = demo_catalog();
        let facts = comparator.compare(GAME_PGN, &catalog).unwrap();
        let affected = facts
            .into_iter()
            .map(|fact| fact.target)
            .collect::<Vec<_>>();
        assert_eq!(affected, vec![TargetId::from("r1:p1")]);
        assert!(catalog.items.contains_key(&affected[0]));
    }
}
