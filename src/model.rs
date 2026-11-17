use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};
use tempo_study::Exercise;
use tempo_types::{ContentVersion, TargetId};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PositionKey(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentKind {
    Repertoire {
        repertoire: String,
        position: PositionKey,
    },
    Tactic {
        practice_start: PositionKey,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessAnswer {
    pub uci: String,
}
impl From<&str> for ChessAnswer {
    fn from(value: &str) -> Self {
        Self { uci: value.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub fen: String,
    pub expected: ChessAnswer,
    pub autoplay: Vec<ChessAnswer>,
}

/// Persisted algorithm identity; unknown serialized variants are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradingScheme {
    ExactSequenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessDefinition {
    pub scheme: GradingScheme,
    pub kind: ContentKind,
    pub decisions: Vec<Decision>,
}
impl ChessDefinition {
    pub fn is_repertoire(&self) -> bool {
        matches!(self.kind, ContentKind::Repertoire { .. })
    }
    pub fn validate(&self) -> Result<(), ChessError> {
        if self.decisions.is_empty() || (self.is_repertoire() && self.decisions.len() != 1) {
            return Err(ChessError::InvalidContent(
                "one repertoire decision or a nonempty tactic is required".into(),
            ));
        }
        for decision in &self.decisions {
            if decision.fen.split_whitespace().count() != 6
                || !valid_uci(&decision.expected.uci)
                || decision.autoplay.iter().any(|m| !valid_uci(&m.uci))
            {
                return Err(ChessError::InvalidContent(
                    "expected six FEN fields and UCI-shaped moves; legality is not checked".into(),
                ));
            }
        }
        Ok(())
    }
}
pub(crate) fn valid_uci(value: &str) -> bool {
    let v = value.as_bytes();
    (v.len() == 4 || (v.len() == 5 && b"qrbn".contains(&v[4])))
        && (b'a'..=b'h').contains(&v[0])
        && (b'1'..=b'8').contains(&v[1])
        && (b'a'..=b'h').contains(&v[2])
        && (b'1'..=b'8').contains(&v[3])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessState {
    pub decision_index: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessPrompt {
    pub fen: String,
    pub instruction: String,
    pub decision_index: usize,
    pub decision_count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessFeedback {
    pub correct: bool,
    pub message: String,
    pub autoplay: Vec<ChessAnswer>,
}
pub type ChessAttempt = tempo_study::Attempt<ChessState, ChessPrompt>;
pub type ChessProgress = tempo_study::Progress<ChessState, ChessPrompt, ChessFeedback>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentStatus {
    Active,
    Quarantined,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentItem {
    pub target: TargetId,
    pub version: ContentVersion,
    pub label: String,
    pub status: ContentStatus,
    pub definition: ChessDefinition,
    pub provenance: Vec<String>,
}
impl ContentItem {
    pub fn exercise(&self) -> Exercise<ChessDefinition> {
        Exercise {
            target: self.target.clone(),
            version: self.version,
            definition: self.definition.clone(),
        }
    }
    pub fn active(&self) -> bool {
        self.status == ContentStatus::Active
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireLine {
    pub id: String,
    pub repertoire: String,
    /// Ordered occurrences, deliberately allowing a target to appear repeatedly.
    pub targets: Vec<TargetId>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub items: BTreeMap<TargetId, ContentItem>,
    pub lines: Vec<RepertoireLine>,
}
impl Catalog {
    pub fn validate(&self) -> Result<(), ChessError> {
        let mut decisions = BTreeMap::new();
        for (key, item) in &self.items {
            if key != &item.target || key.0.is_empty() || !key.0.is_ascii() || item.version.0 == 0 {
                return Err(ChessError::InvalidContent(
                    "target identity/version is invalid".into(),
                ));
            }
            item.definition.validate()?;
            match &item.definition.kind {
                ContentKind::Repertoire {
                    repertoire,
                    position,
                } => {
                    if repertoire.is_empty() || position.0.is_empty() {
                        return Err(ChessError::InvalidContent(
                            "repertoire and position identity required".into(),
                        ));
                    }
                    if decisions.insert((repertoire, position), key).is_some() {
                        return Err(ChessError::ConflictingResponse {
                            repertoire: repertoire.clone(),
                            position: position.clone(),
                        });
                    }
                }
                ContentKind::Tactic { practice_start } if practice_start.0.is_empty() => {
                    return Err(ChessError::InvalidContent(
                        "tactic start identity required".into(),
                    ));
                }
                ContentKind::Tactic { .. } => {}
            }
        }
        let mut line_ids = BTreeSet::new();
        for line in &self.lines {
            if line.id.is_empty() || !line_ids.insert(&line.id) {
                return Err(ChessError::InvalidContent(
                    "line identifiers must be unique and nonempty".into(),
                ));
            }
            for target in &line.targets {
                let item = self
                    .items
                    .get(target)
                    .ok_or_else(|| ChessError::UnknownTarget(target.clone()))?;
                if !matches!(&item.definition.kind, ContentKind::Repertoire { repertoire, .. } if repertoire == &line.repertoire)
                {
                    return Err(ChessError::InvalidContent(
                        "line target belongs to another repertoire".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChessError {
    InvalidContent(String),
    UnknownFixture(String),
    UnknownTarget(TargetId),
    ConflictingResponse {
        repertoire: String,
        position: PositionKey,
    },
    IdentityChanged(TargetId),
    VersionExhausted(TargetId),
}
impl fmt::Display for ChessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for ChessError {}
