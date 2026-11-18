//! ChessError and its usage scenarios.

use crate::PositionKey;
use std::{error::Error, fmt};
use tempo_types::TargetId;

/// Failures to construct, identify, import or resolve chess content.
///
/// # When to use
///
/// Parsers, canonicalizers, comparators, catalog validation and import planning
/// return these errors to the application. A conflict identifies content that
/// needs attention; it is not a learner's `Again` assessment.
///
/// `UnknownFixture` means a registered-input substitute lacks an implementation
/// for that input. It does not mean the input is invalid PGN or an illegal FEN.
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
        match self {
            Self::InvalidContent(message) | Self::UnknownFixture(message) => f.write_str(message),
            Self::UnknownTarget(target) => write!(f, "unknown target {}", target.0),
            Self::ConflictingResponse {
                repertoire,
                position,
            } => write!(
                f,
                "conflicting learner responses in {repertoire} at position {}",
                position.0
            ),
            Self::IdentityChanged(target) => write!(f, "target identity changed for {}", target.0),
            Self::VersionExhausted(target) => {
                write!(f, "content version exhausted for target {}", target.0)
            }
        }
    }
}
impl Error for ChessError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GAME_PGN, GameComparator, demo_catalog, demo_comparator};

    #[test]
    fn a_comparison_identifies_the_missing_target_instead_of_inventing_a_grade() {
        let mut catalog = demo_catalog();
        catalog.items.remove(&TargetId::from("r1:p1"));
        let error = demo_comparator().compare(GAME_PGN, &catalog).unwrap_err();
        match error {
            ChessError::UnknownTarget(target) => assert_eq!(target, TargetId::from("r1:p1")),
            other => panic!("expected a missing content reference, got {other}"),
        }
    }
}
