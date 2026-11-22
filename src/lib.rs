//! Chess content contracts, prescribed-move grading and explicit fixtures.
//!
//! Start with [`Catalog`], then [`ContentItem`] and [`ChessDefinition`]. Each
//! named module owns one concept and its usage scenarios. For standard chess
//! positions and repertoire PGNs, start with the two concrete import adapters.

mod apply_setup_move;
mod authored_repertoire;
mod marked_lines;
pub use marked_lines::{MarkedDraft, MoveReplacement, draft_marked_lines};
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
    AuthoredRepertoire, CoverageLine, CoverageStart, DepartureKind, GapDraft, LearnerResponse,
    OpponentGap, RepertoireAdherence, legal_model_moves, legal_successors,
};
mod game_detector;
mod hanging_capture;
mod hanging_material;
mod personal_tactics;
mod tactic_explanation;
mod tactic_payoff;
pub use tactic_payoff::ambiguous_hanging_material;
pub use tactic_payoff::{PayoffMotif, TacticalPayoff, recognizable_payoff, tactical_payoff};
mod tactical_evaluation;
mod tactical_puzzle;
pub use game_detector::{
    BoardSubject, DetectedMoment, DetectionControl, DetectionOutcome, DetectionStop,
    DetectionValidationError, DetectorDescriptor, DetectorIdentity, GameDetectionInput,
    GameDetector, PieceRole, PositionFinding, PositionPass, TacticalEvidenceFormat,
    ValidatedMoment, scan_position, validate_detected_moments,
};
pub use hanging_material::{
    CaptureQuality, HangingCapture, HangingCaptureAssessment, HangingMaterialConfig,
    HangingMaterialDetector, TacticalAssessment, TacticalKind, TacticalOutcome,
    TacticalPuzzleSolution, hanging_captures,
};
pub use personal_tactics::{
    Motif, MotifCandidate, PersonalPuzzle, PersonalPuzzleKind, PersonalTacticsDetector,
    PersonalTacticsEvidence, TacticRecognition, recognition_roots, recognize_game_tactics,
};
pub use tactic_explanation::{
    TacticExplanation, explain_issued_tactic, explain_tactic_decision, explain_tactic_move,
};
pub use tactical_evaluation::{
    EvaluationFailure, EvaluationProvenance, EvaluationScore, EvaluationStatus, RootEvaluation,
    RootScore, ScoreBound, TacticalEvaluationRequest, TacticalEvaluator,
};
pub use tactical_puzzle::{
    GameTacticQualification, GeneratedTacticExtension, GeneratedTacticalPuzzle,
    QualifiedTacticalPuzzle, TacticalPuzzlePlan, game_generated_definition, plan_personal_puzzle,
    plan_tactical_puzzle, prepare_personal_puzzle, prepare_tactical_puzzle, requalify_game_tactic,
};
mod played_game_timeline;
pub use played_game_timeline::{PlayedGameTimeline, PositionOccurrence};
mod played_opening;
pub use played_opening::{PlayedOpening, played_opening};
mod played_game;
pub use played_game::{
    DecisionAdherence, PlayedDecision, PlayedGame, RepertoireComparison, RepertoireObservation,
    compare_active_repertoires, compare_authored_repertoires, compare_played_game,
    opponent_deviations, parse_played_games, played_deviation, played_deviation_lead_in,
};
mod grading_scheme;
mod import_request;
mod learner_color;
mod legal_sequence_grader;
mod opponent_lead_in;
mod parsed_pgn;
mod pgn_tokens;
mod plan_import;
mod plan_repertoire_replacement;
mod plan_tactic_capture;
mod position_canonicalizer;
mod position_display;
mod revealed_line;
pub use revealed_line::{
    RevealedLine, RevealedMove, first_matching_repertoire_line, revealed_attempt_line,
    revealed_line,
};
mod position_key;
mod repertoire_lesson;
mod repertoire_line;
mod repertoire_pgn_parser;
mod repertoire_study_route;
mod standard_position_canonicalizer;
mod structure;
pub use structure::{
    NearbyPosition, NearbyRepertoirePositions, NearbyResponse, PositionStructure,
    SavedRepertoireResponse, nearby_repertoire_positions, structural_neighbors,
};
mod standard_uci;
mod tactic_pgn;
mod tactic_policy_facts;
mod tactic_record;
mod tactic_themes;

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
pub use content_item::{ContentItem, GameCapture};
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
pub use legal_sequence_grader::{GuidedMoveError, LegalSequenceGrader, acknowledge_shown_move};
pub use opponent_lead_in::{LeadInEvidence, OpponentLeadIn, archived_tactic_lead_in};
pub use plan_import::plan_import;
pub use plan_repertoire_replacement::plan_repertoire_replacement;
pub use plan_tactic_capture::{
    TacticCapturePlan, TacticIdentity, TacticLookup, plan_tactic_capture,
};
pub use position_canonicalizer::PositionCanonicalizer;
pub use position_display::{PositionDisplay, legal_move_labels, move_san, project_position};
pub use position_key::PositionKey;
pub use repertoire_lesson::RepertoireLesson;
pub use repertoire_line::RepertoireLine;
pub use repertoire_pgn_parser::RepertoirePgnParser;
pub use repertoire_study_route::{RepertoireStudyStep, repertoire_study_routes};
pub use standard_position_canonicalizer::StandardPositionCanonicalizer;
pub use tactic_pgn::{
    TacticPgnMove, TacticPgnPreview, TacticPgnRoute, TacticPgnRow, TacticPgnSelection,
    TacticPgnStatus, preview_tactic_pgn,
};
pub use tactic_policy_facts::TacticPolicyFacts;
pub use tactic_record::TacticRecord;
pub use tactic_themes::{TacticTheme, known_tactic_theme, tactic_themes};

mod game_report_facts;
pub use game_report_facts::{
    GameReportFacts, GameTimeControl, LearnerGameResult, game_report_facts,
};

mod opening_partition;
pub use opening_partition::partition_black_openings;
