//! Chess content contracts, prescribed-move grading and explicit fixtures.
//!
//! Start with [`Catalog`], then [`ContentItem`] and [`ChessDefinition`]. Each
//! named module owns one concept and its usage scenarios. For standard chess
//! positions and repertoire PGNs, start with the two concrete import adapters.

mod catalog;
mod change_kind;
mod chess_answer;
mod chess_attempt;
mod chess_definition;
mod chess_error;
mod chess_feedback;
mod chess_progress;
mod chess_prompt;
mod chess_state;
mod content_change;
mod content_item;
mod content_kind;
mod content_parser;
mod content_plan;
mod content_status;
mod decision;
mod demo_fixtures;
mod deviation;
mod exact_sequence_grader;
mod fixture_canonicalizer;
mod fixture_comparator;
mod fixture_parser;
mod game_comparator;
mod grading_scheme;
mod import_request;
mod learner_color;
mod pgn_tokens;
mod plan_import;
mod position_canonicalizer;
mod position_key;
mod repertoire_line;
mod repertoire_pgn_parser;
mod standard_position_canonicalizer;

pub use catalog::Catalog;
pub use change_kind::ChangeKind;
pub use chess_answer::ChessAnswer;
pub use chess_attempt::ChessAttempt;
pub use chess_definition::ChessDefinition;
pub use chess_error::ChessError;
pub use chess_feedback::ChessFeedback;
pub use chess_progress::ChessProgress;
pub use chess_prompt::ChessPrompt;
pub use chess_state::ChessState;
pub use content_change::ContentChange;
pub use content_item::ContentItem;
pub use content_kind::ContentKind;
pub use content_parser::ContentParser;
pub use content_plan::ContentPlan;
pub use content_status::ContentStatus;
pub use decision::Decision;
pub use demo_fixtures::{
    GAME_PGN, REPERTOIRE_PGN, SECOND_FEN, START_FEN, TACTIC_PGN, demo_canonicalizer, demo_catalog,
    demo_comparator, demo_parser,
};
pub use deviation::Deviation;
pub use exact_sequence_grader::ExactSequenceGrader;
pub use fixture_canonicalizer::FixtureCanonicalizer;
pub use fixture_comparator::FixtureComparator;
pub use fixture_parser::FixtureParser;
pub use game_comparator::GameComparator;
pub use grading_scheme::GradingScheme;
pub use import_request::ImportRequest;
pub use learner_color::LearnerColor;
pub use plan_import::plan_import;
pub use position_canonicalizer::PositionCanonicalizer;
pub use position_key::PositionKey;
pub use repertoire_line::RepertoireLine;
pub use repertoire_pgn_parser::RepertoirePgnParser;
pub use standard_position_canonicalizer::StandardPositionCanonicalizer;
