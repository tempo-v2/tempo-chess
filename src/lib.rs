//! Chess content contracts and explicit fixture implementations.
//! No chess engine, general PGN parser, or general FEN canonicalizer is implemented here.
mod changes;
mod fixtures;
mod grading;
mod model;
mod ports;
#[cfg(test)]
mod tests;

pub use changes::{ChangeKind, ContentChange, ContentPlan, plan_import};
pub use fixtures::{
    GAME_PGN, REPERTOIRE_PGN, SECOND_FEN, START_FEN, TACTIC_PGN, demo_canonicalizer, demo_catalog,
    demo_comparator, demo_parser,
};
pub use grading::ExactSequenceGrader;
pub use model::{
    Catalog, ChessAnswer, ChessAttempt, ChessDefinition, ChessError, ChessFeedback, ChessProgress,
    ChessPrompt, ChessState, ContentItem, ContentKind, ContentStatus, Decision, GradingScheme,
    PositionKey, RepertoireLine,
};
pub use ports::{
    ContentParser, Deviation, FixtureCanonicalizer, FixtureComparator, FixtureParser,
    GameComparator, ImportRequest, PositionCanonicalizer,
};
