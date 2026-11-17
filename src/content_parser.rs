//! ContentParser and its usage scenarios.

use crate::{Catalog, ChessError, ImportRequest};

/// The replaceable PGN-to-content boundary.
///
/// # When to use
///
/// An application import command passes an [`ImportRequest`] and receives a
/// proposed [`Catalog`]. It then calls [`crate::plan_import`] against current
/// content. Parsing does not persist content, overwrite learning or decide an
/// application transaction.
///
/// [`crate::FixtureParser`] registers exact inputs for wiring tests. A production
/// parser can implement this trait without changing the import command's shape.
///
/// ```
/// use tempo_chess::{ContentParser, ImportRequest, demo_parser, REPERTOIRE_PGN};
///
/// fn read_repertoire(parser: &impl ContentParser) -> Result<tempo_chess::Catalog, tempo_chess::ChessError> {
///     parser.parse(&ImportRequest {
///         pgn: REPERTOIRE_PGN.into(),
///         source: "My repertoire export".into(),
///         repertoire: Some("r1".into()),
///     })
/// }
/// let proposed = read_repertoire(&demo_parser())?;
/// assert_eq!(proposed.items.len(), 2);
/// # Ok::<(), tempo_chess::ChessError>(())
/// ```
pub trait ContentParser {
    fn parse(&self, request: &ImportRequest) -> Result<Catalog, ChessError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{REPERTOIRE_PGN, demo_parser, plan_import};

    #[test]
    fn the_application_can_parse_and_plan_through_the_trait_without_a_store() {
        let parser: &dyn ContentParser = &demo_parser();
        let current = Catalog::default();
        let proposed = parser
            .parse(&ImportRequest {
                pgn: REPERTOIRE_PGN.into(),
                source: "My opening file".into(),
                repertoire: Some("r1".into()),
            })
            .unwrap();
        let plan = plan_import(&current, &proposed).unwrap();
        assert_eq!(plan.catalog.items.len(), 2);
        assert!(current.items.is_empty());
    }
}
