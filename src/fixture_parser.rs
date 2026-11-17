//! FixtureParser and its usage scenarios.

use crate::{Catalog, ChessError, ContentKind, ContentParser, ImportRequest};
use std::collections::BTreeMap;

/// A registered-input substitute for a general PGN parser.
///
/// # When to use
///
/// Use this in demos or interface tests that need deterministic parser output
/// before a real parser exists. `with_fixture` maps exact PGN text to a catalog;
/// `parse` clones that catalog, adds request provenance, checks optional repertoire
/// context and validates its structure. Registered catalogs are returned as-is
/// for the learner color; use the real repertoire parser to select a side.
///
/// Unknown input returns [`ChessError::UnknownFixture`]. This implementation does
/// not claim to parse arbitrary valid chess text.
#[derive(Clone, Debug, Default)]
pub struct FixtureParser {
    fixtures: BTreeMap<String, Catalog>,
}
impl FixtureParser {
    pub fn with_fixture(mut self, pgn: impl Into<String>, catalog: Catalog) -> Self {
        self.fixtures.insert(pgn.into(), catalog);
        self
    }
}
impl ContentParser for FixtureParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError> {
        let mut catalog = self.fixtures.get(&request.pgn).cloned().ok_or_else(|| {
            ChessError::UnknownFixture("PGN is not registered in the fixture parser".into())
        })?;
        if let Some(repertoire) = &request.repertoire {
            if catalog.items.values().any(|item| {
                matches!(&item.definition.kind,
                ContentKind::Repertoire { repertoire: expected, .. } if expected != repertoire)
            }) {
                return Err(ChessError::UnknownFixture(
                    "requested repertoire does not match the registered fixture".into(),
                ));
            }
        }
        for item in catalog.items.values_mut() {
            if !item.provenance.contains(&request.source) {
                item.provenance.push(request.source.clone());
            }
        }
        catalog.validate()?;
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{REPERTOIRE_PGN, demo_parser};

    #[test]
    fn registered_pgn_adds_provenance_but_unknown_input_is_an_explicit_fixture_error() {
        let parser = demo_parser();
        let request = ImportRequest {
            pgn: REPERTOIRE_PGN.into(),
            source: "test import".into(),
            repertoire: Some("r1".into()),
            learner_color: None,
        };
        let known = parser.parse(&request).unwrap();
        assert_eq!(known.items.len(), 2);
        assert!(
            known
                .items
                .values()
                .all(|item| item.provenance.contains(&request.source))
        );
        assert!(matches!(
            parser.parse(&ImportRequest {
                pgn: "1. a3 *".into(),
                ..request
            }),
            Err(ChessError::UnknownFixture(_))
        ));
    }
}
