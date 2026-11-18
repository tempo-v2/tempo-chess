//! ImportRequest and its usage scenarios.

use crate::LearnerColor;
use serde::{Deserialize, Serialize};

/// The PGN text and import context supplied to a content parser.
///
/// # When to use
///
/// The application constructs a request after obtaining PGN bytes. A
/// [`crate::ContentParser`] uses the source label as provenance and the optional
/// repertoire identifier as context. File/provider I/O happens before this boundary.
///
/// A missing identifier permits content without repertoire context, such as
/// tactics. The fixture parser checks any supplied identifier against the
/// registered repertoire. [`crate::RepertoirePgnParser`] requires its configured
/// destination identifier and a learner color.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub pgn: String,
    pub source: String,
    /// Stable repertoire context for the selected parser.
    pub repertoire: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learner_color: Option<LearnerColor>,
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
            learner_color: None,
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

    #[test]
    fn legacy_requests_round_trip_and_explicit_colors_use_lowercase() {
        let old = serde_json::json!({"pgn": "1. e4 *", "source": "book", "repertoire": "r1"});
        let mut request: ImportRequest = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(request.learner_color, None);
        assert_eq!(serde_json::to_value(&request).unwrap(), old);
        for (color, spelling) in [
            (LearnerColor::White, "white"),
            (LearnerColor::Black, "black"),
        ] {
            request.learner_color = Some(color);
            let wire = serde_json::to_value(&request).unwrap();
            assert_eq!(wire["learner_color"], spelling);
            assert_eq!(
                serde_json::from_value::<ImportRequest>(wire).unwrap(),
                request
            );
        }
    }
}
