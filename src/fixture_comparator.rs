//! FixtureComparator and its usage scenarios.

use crate::{Catalog, ChessError, Deviation, GameComparator};
use std::collections::BTreeMap;

/// A registered-game substitute for real repertoire comparison.
///
/// # When to use
///
/// Use `with_fixture` to associate exact game text with known deviations. Comparing
/// that game checks that all referenced targets exist in the supplied catalog.
/// This lets application tests exercise feedback without network or parsing work.
///
/// Unknown game text returns an explicit error. A registered deviation for a
/// missing target is also an error, rather than feedback against unrelated content.
#[derive(Clone, Debug, Default)]
pub struct FixtureComparator {
    fixtures: BTreeMap<String, Vec<Deviation>>,
}
impl FixtureComparator {
    pub fn with_fixture(mut self, pgn: impl Into<String>, deviations: Vec<Deviation>) -> Self {
        self.fixtures.insert(pgn.into(), deviations);
        self
    }
}
impl GameComparator for FixtureComparator {
    fn compare(&self, pgn: &str, catalog: &Catalog) -> Result<Vec<Deviation>, ChessError> {
        let deviations = self.fixtures.get(pgn).ok_or_else(|| {
            ChessError::UnknownFixture("game is not registered in the fixture comparator".into())
        })?;
        for deviation in deviations {
            if !catalog.items.contains_key(&deviation.target) {
                return Err(ChessError::UnknownTarget(deviation.target.clone()));
            }
        }
        Ok(deviations.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GAME_PGN, demo_catalog, demo_comparator};
    use tempo_types::TargetId;

    #[test]
    fn registered_games_require_references_to_content_that_actually_exists() {
        let comparator = demo_comparator();
        let mut catalog = demo_catalog();
        assert_eq!(comparator.compare(GAME_PGN, &catalog).unwrap().len(), 1);
        assert!(matches!(
            comparator.compare("unknown game", &catalog),
            Err(ChessError::UnknownFixture(_))
        ));
        catalog.items.remove(&TargetId::from("r1:p1"));
        assert_eq!(
            comparator.compare(GAME_PGN, &catalog),
            Err(ChessError::UnknownTarget("r1:p1".into()))
        );
    }
}
