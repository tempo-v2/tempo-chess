//! ImportRequest and its usage scenarios.

use serde::{Deserialize, Serialize};

/// The PGN text and import context supplied to a content parser.
///
/// # When to use
///
/// The application constructs a request after obtaining PGN bytes. A
/// [`crate::ContentParser`] uses the source label as provenance and the optional
/// repertoire name as context. File/provider I/O happens before this boundary.
///
/// A missing repertoire name permits content without repertoire context, such as
/// tactics. In the fixture parser, a supplied name must match the registered
/// repertoire; the parser does not silently rename targets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub pgn: String,
    pub source: String,
    pub repertoire: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChessError, ContentParser, REPERTOIRE_PGN, demo_parser};

    #[test]
    fn explicit_repertoire_context_prevents_importing_into_the_wrong_repertoire() {
        let request = ImportRequest {
            pgn: REPERTOIRE_PGN.into(),
            source: "Annotated opening export".into(),
            repertoire: Some("different-repertoire".into()),
        };
        assert!(matches!(
            demo_parser().parse(&request),
            Err(ChessError::UnknownFixture(_))
        ));
        let corrected = ImportRequest {
            repertoire: Some("r1".into()),
            ..request
        };
        let proposed = demo_parser().parse(&corrected).unwrap();
        assert!(
            proposed
                .items
                .values()
                .all(|item| item.provenance.contains(&corrected.source))
        );
    }
}
