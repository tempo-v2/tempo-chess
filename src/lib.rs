//! Chess content contracts, prescribed-move grading and explicit fixtures.
//!
//! Start with [`Catalog`], then [`ContentItem`] and [`ChessDefinition`]. Each
//! named module owns one concept and its usage scenarios. For standard chess
//! positions and repertoire PGNs, start with the two concrete import adapters.

mod apply_setup_move;
mod authored_repertoire;
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
mod compile_repertoire;
mod compile_tactics;
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
pub use authored_repertoire::{
    AuthoredRepertoire, DepartureKind, GapDraft, OpponentGap, RepertoireAdherence,
    legal_model_moves,
};
mod played_game;
pub use played_game::{
    DecisionAdherence, PlayedDecision, PlayedGame, RepertoireComparison, RepertoireObservation,
    compare_active_repertoires, compare_authored_repertoires, compare_played_game,
    opponent_deviations, parse_played_games, played_deviation,
};
mod grading_scheme;
mod import_request;
mod learner_color;
mod legal_sequence_grader;
mod parsed_pgn;
mod pgn_tokens;
mod plan_import;
mod plan_repertoire_replacement;
mod plan_tactic_capture;
mod position_canonicalizer;
mod position_display;
mod revealed_line;
pub use revealed_line::{RevealedLine, RevealedMove, revealed_line};
mod position_key;
mod repertoire_lesson;
mod repertoire_line;
mod repertoire_pgn_parser;
mod standard_position_canonicalizer;
mod structure;
pub use structure::structural_neighbors;
mod standard_uci;
mod tactic_pgn;
mod tactic_record;

pub use apply_setup_move::apply_setup_move;
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
pub use compile_repertoire::compile_repertoire;
pub use compile_tactics::compile_tactics;
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
pub use legal_sequence_grader::LegalSequenceGrader;
pub use plan_import::plan_import;
pub use plan_repertoire_replacement::plan_repertoire_replacement;
pub use plan_tactic_capture::{TacticCapturePlan, plan_tactic_capture};
pub use position_canonicalizer::PositionCanonicalizer;
pub use position_display::{PositionDisplay, legal_move_labels, project_position};
pub use position_key::PositionKey;
pub use repertoire_lesson::RepertoireLesson;
pub use repertoire_line::RepertoireLine;
pub use repertoire_pgn_parser::RepertoirePgnParser;
pub use standard_position_canonicalizer::StandardPositionCanonicalizer;
pub use tactic_pgn::{
    TacticPgnMove, TacticPgnPreview, TacticPgnRoute, TacticPgnRow, TacticPgnSelection,
    TacticPgnStatus, preview_tactic_pgn,
};
pub use tactic_record::TacticRecord;
