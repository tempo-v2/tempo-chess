use crate::{Catalog, ChessError, ContentKind, PositionKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tempo_types::TargetId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub pgn: String,
    pub source: String,
    pub repertoire: Option<String>,
}
pub trait ContentParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError>;
}
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

pub trait PositionCanonicalizer {
    fn canonicalize(&self, fen: &str) -> Result<PositionKey, ChessError>;
}
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deviation {
    pub target: TargetId,
    pub ply: u32,
    pub observed_move: String,
}
pub trait GameComparator {
    fn compare(&self, pgn: &str, catalog: &Catalog) -> Result<Vec<Deviation>, ChessError>;
}
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
