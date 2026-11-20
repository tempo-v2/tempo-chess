//! A read-only detector receives one mainline and the app-resolved learner color.

use crate::standard_uci::legal_move;
use crate::tactical_evaluation::{checked_evaluation, evaluation_request, validate_winning_mate};
use crate::{ChessAnswer, LearnerColor, PlayedGameTimeline, PositionKey, TacticalAssessment};
use cozy_chess::util::display_san_move;
use cozy_chess::{Board, Move, Piece, Square};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeSet;

/// Immutable chess inputs to a detector. The host resolves the learner before
/// creating this value. Either side can be to move at any occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameDetectionInput {
    timeline: PlayedGameTimeline,
    learner: LearnerColor,
}

impl GameDetectionInput {
    pub fn new(timeline: PlayedGameTimeline, learner: LearnerColor) -> Self {
        Self { timeline, learner }
    }

    pub fn timeline(&self) -> &PlayedGameTimeline {
        &self.timeline
    }

    pub fn learner(&self) -> LearnerColor {
        self.learner
    }

    /// Versioned hex BLAKE3 digest of all exposed semantic inputs. PGN tags,
    /// comments and variations never enter the timeline or this digest.
    pub fn fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"tempo-chess/detector-input/v1");
        hash.update(&[color_code(self.learner)]);
        let occurrences = self.timeline.occurrences();
        hash.update(&(occurrences.len() as u64).to_le_bytes());
        for occurrence in occurrences {
            hash.update(&(occurrence.offset() as u64).to_le_bytes());
            hash_text(&mut hash, occurrence.fen());
            hash_text(&mut hash, &occurrence.position().0);
            hash.update(&[color_code(occurrence.side_to_move())]);
            hash_candidate(&mut hash, occurrence.next_move());
        }
        hash.finalize().to_hex().to_string()
    }
}

/// The host registers descriptors by unique stable ID. Revision and
/// configuration identify the implementation, and the label is for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DetectorDescriptor {
    pub id: String,
    pub revision: String,
    pub configuration_fingerprint: String,
    pub label: String,
}

/// Durable detector identity. Presentation labels do not identify work or evidence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DetectorIdentity {
    pub id: String,
    pub revision: String,
    pub configuration_fingerprint: String,
}

impl DetectorDescriptor {
    pub fn identity(&self) -> DetectorIdentity {
        DetectorIdentity {
            id: self.id.clone(),
            revision: self.revision.clone(),
            configuration_fingerprint: self.configuration_fingerprint.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), DetectionValidationError> {
        if [&self.id, &self.revision, &self.configuration_fingerprint]
            .iter()
            .any(|part| part.trim().is_empty())
        {
            return Err(DetectionValidationError::InvalidDescriptor);
        }
        Ok(())
    }
}

/// The durable evidence contract a position detector implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TacticalEvidenceFormat {
    LegacyFindings,
    CompleteDecision,
}

/// Inject a detector at the host composition boundary. Implementations must
/// check control between occurrences and during any nontrivial inner loops.
pub trait GameDetector: Send + Sync {
    fn descriptor(&self) -> DetectorDescriptor;
    fn detect(
        &self,
        input: &GameDetectionInput,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome;
    /// Position findings carry a typed tactical assessment that the durable
    /// game-analysis consumer can validate and retain.
    fn produces_tactical_assessments(&self) -> bool {
        false
    }

    /// Complete decisions cover all motifs before the host accepts a turn.
    fn tactical_evidence_format(&self) -> Option<TacticalEvidenceFormat> {
        self.produces_tactical_assessments()
            .then_some(TacticalEvidenceFormat::LegacyFindings)
    }

    /// Analyze one addressed position in the validated timeline.
    fn detect_position(
        &self,
        _position: &PositionPass<'_>,
        _control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        DetectionOutcome::Unsupported
    }
}

/// Validated position and engine comparisons scoped to one exact occurrence.
/// The same instance is passed to every requested detector by `scan_position`.
pub struct PositionPass<'a> {
    input: &'a GameDetectionInput,
    ply: usize,
    pub(crate) board: &'a Board,
    pub(crate) legal: Vec<Move>,
    legal_answers: Vec<ChessAnswer>,
    comparisons: RefCell<
        Vec<(
            crate::EvaluationProvenance,
            Result<crate::EvaluationStatus, DetectionStop>,
        )>,
    >,
}

impl<'a> PositionPass<'a> {
    pub(crate) fn new(input: &'a GameDetectionInput, ply: usize) -> Option<Self> {
        let board = &input.timeline().occurrences().get(ply)?.board;
        let mut legal = Vec::new();
        board.generate_moves(|moves| {
            legal.extend(moves);
            false
        });
        Some(Self {
            input,
            ply,
            board,
            legal_answers: legal
                .iter()
                .map(|&mv| {
                    ChessAnswer::from(
                        cozy_chess::util::display_uci_move(board, mv)
                            .to_string()
                            .as_str(),
                    )
                })
                .collect(),
            legal,
            comparisons: RefCell::new(Vec::new()),
        })
    }

    pub fn occurrence(&self) -> &crate::PositionOccurrence {
        &self.input.timeline().occurrences()[self.ply]
    }

    pub fn learner(&self) -> LearnerColor {
        self.input.learner()
    }

    pub(crate) fn earlier_boards(&self) -> impl Iterator<Item = &Board> {
        self.input.timeline().occurrences()[..self.ply]
            .iter()
            .rev()
            .map(|occurrence| &occurrence.board)
    }

    /// The validated moves already played, in order. The current move is
    /// available separately on the occurrence.
    pub fn played_prefix(&self) -> impl ExactSizeIterator<Item = &ChessAnswer> {
        self.input.timeline().occurrences()[..self.ply]
            .iter()
            .map(|occurrence| occurrence.next_move().expect("nonterminal played prefix"))
    }

    pub fn legal_moves(&self) -> &[ChessAnswer] {
        &self.legal_answers
    }

    pub fn piece_at(&self, square: &str) -> Option<BoardSubject> {
        let address: Square = square.parse().ok()?;
        let role = PieceRole::from_piece(self.board.piece_on(address)?);
        let color = LearnerColor::from_board(self.board.color_on(address)?);
        Some(BoardSubject {
            square: address.to_string(),
            role,
            color,
        })
    }

    pub fn move_is_mate(&self, uci: &str) -> bool {
        let Some(index) = self
            .legal_answers
            .iter()
            .position(|answer| answer.uci == uci)
        else {
            return false;
        };
        let mut after = self.board.clone();
        after.play_unchecked(self.legal[index]);
        after.status() == cozy_chess::GameStatus::Won
    }

    /// Compare all legal roots against the unchanged game prefix, sharing the
    /// result only with the same evaluator profile in this position pass.
    pub(crate) fn engine_request(&self) -> crate::TacticalEvaluationRequest {
        evaluation_request(self.input, self.ply, &self.legal)
    }

    pub fn compare(
        &self,
        evaluator: &dyn crate::TacticalEvaluator,
        control: &dyn DetectionControl,
    ) -> Result<crate::EvaluationStatus, DetectionStop> {
        control.check()?;
        let profile = evaluator.descriptor();
        if let Some((_, cached)) = self
            .comparisons
            .borrow()
            .iter()
            .find(|(key, _)| key == &profile)
        {
            return cached.clone();
        }
        let request = self.engine_request();
        let result = checked_evaluation(evaluator, &request, control);
        self.comparisons
            .borrow_mut()
            .push((profile, result.clone()));
        result
    }
}

/// A detector claim paired with its producer before the host validates it.
pub struct PositionFinding {
    pub producer: DetectorIdentity,
    pub moment: DetectedMoment,
    pub skipped: Vec<DetectorIdentity>,
}

/// One shared board and search context for the entire requested detector set.
pub fn scan_position<'a>(
    input: &GameDetectionInput,
    ply: usize,
    detectors: impl IntoIterator<Item = &'a dyn GameDetector>,
    control: &dyn DetectionControl,
) -> DetectionOutcome<PositionFinding> {
    let Some(pass) = PositionPass::new(input, ply) else {
        return DetectionOutcome::Failed("Invalid position occurrence.".into());
    };
    let mut findings = Vec::new();
    let detectors: Vec<_> = detectors.into_iter().collect();
    let complete_decision = detectors.iter().any(|detector| {
        detector.tactical_evidence_format() == Some(TacticalEvidenceFormat::CompleteDecision)
    });
    let mut detectors = detectors.into_iter();
    while let Some(detector) = detectors.next() {
        match detector.detect_position(&pass, control) {
            DetectionOutcome::Complete(moments) => {
                if !complete_decision
                    && let Some(mate) = moments.iter().find(|moment| {
                        moment
                            .tactical
                            .as_ref()
                            .is_some_and(|assessment| validate_winning_mate(&pass, assessment))
                    })
                {
                    return DetectionOutcome::Complete(vec![PositionFinding {
                        producer: detector.descriptor().identity(),
                        moment: mate.clone(),
                        skipped: detectors
                            .map(|detector| detector.descriptor().identity())
                            .collect(),
                    }]);
                }
                let producer = detector.descriptor().identity();
                findings.extend(moments.into_iter().map(|moment| PositionFinding {
                    producer: producer.clone(),
                    moment,
                    skipped: vec![],
                }));
            }
            DetectionOutcome::Unsupported => return DetectionOutcome::Unsupported,
            DetectionOutcome::Stopped(reason) => return DetectionOutcome::Stopped(reason),
            DetectionOutcome::Failed(error) => return DetectionOutcome::Failed(error),
        }
    }
    DetectionOutcome::Complete(findings)
}

/// The host communicates cancellation and budget exhaustion through this port.
pub trait DetectionControl {
    fn check(&self) -> Result<(), DetectionStop>;
    fn deeper_search(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionStop {
    Cancelled,
    Deadline,
}

/// Only Complete carries findings, including the valid empty report.
#[derive(Debug, PartialEq, Eq)]
pub enum DetectionOutcome<F = DetectedMoment> {
    Complete(Vec<F>),
    Unsupported,
    Stopped(DetectionStop),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum PieceRole {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

impl PieceRole {
    pub(crate) fn from_piece(piece: Piece) -> Self {
        match piece {
            Piece::Pawn => Self::Pawn,
            Piece::Knight => Self::Knight,
            Piece::Bishop => Self::Bishop,
            Piece::Rook => Self::Rook,
            Piece::Queen => Self::Queen,
            Piece::King => Self::King,
        }
    }

    fn piece(self) -> Piece {
        match self {
            Self::Pawn => Piece::Pawn,
            Self::Knight => Piece::Knight,
            Self::Bishop => Piece::Bishop,
            Self::Rook => Piece::Rook,
            Self::Queen => Piece::Queen,
            Self::King => Piece::King,
        }
    }
}

/// A claim about the occupant of an orthodox square at the finding's anchor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardSubject {
    pub square: String,
    pub role: PieceRole,
    pub color: LearnerColor,
}

/// Detector claims before checking against the input. The explanation is
/// presentation text and does not participate in identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedMoment {
    pub occurrence: usize,
    pub code: String,
    pub subjects: Vec<BoardSubject>,
    pub candidate_move: Option<String>,
    pub explanation: String,
    pub tactical: Option<TacticalAssessment>,
}

/// Checked claims and board facts supplied by Chess. SAN is derived from the
/// actual anchor and the checked standard UCI candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedMoment {
    pub occurrence: usize,
    pub code: String,
    pub subjects: Vec<BoardSubject>,
    pub candidate_move: Option<ChessAnswer>,
    pub candidate_san: Option<String>,
    pub explanation: String,
    pub fen: String,
    pub position: PositionKey,
    pub side_to_move: LearnerColor,
    #[serde(skip)]
    pub opponent_lead_in: Option<crate::OpponentLeadIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tactical: Option<TacticalAssessment>,
}

impl ValidatedMoment {
    /// Local finding identity. The application must scope this digest to the
    /// game, input fingerprint and registered detector before exposing an ID.
    pub fn semantic_fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"tempo-chess/detected-moment/v1");
        hash.update(&(self.occurrence as u64).to_le_bytes());
        hash_text(&mut hash, &self.code);
        hash.update(&(self.subjects.len() as u64).to_le_bytes());
        for subject in &self.subjects {
            hash_text(&mut hash, &subject.square);
            hash.update(&[subject.role as u8, color_code(subject.color)]);
        }
        hash_candidate(&mut hash, self.candidate_move.as_ref());
        if let Some(tactical) = &self.tactical {
            hash.update(b"tactical/v1");
            hash_text(
                &mut hash,
                &serde_json::to_string(tactical).expect("typed tactical evidence"),
            );
        }
        hash.finalize().to_hex().to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionValidationError {
    InvalidDescriptor,
    InvalidOccurrence,
    InvalidCode,
    InvalidSubject,
    InvalidCandidateMove,
    DuplicateFinding,
    InvalidTacticalAssessment,
}

/// Validate the entire Complete report before it can be published. An invalid
/// row rejects the report. This verifies chess facts, not the detector's diagnosis.
pub fn validate_detected_moments(
    input: &GameDetectionInput,
    findings: Vec<DetectedMoment>,
) -> Result<Vec<ValidatedMoment>, DetectionValidationError> {
    let mut seen = BTreeSet::new();
    findings
        .into_iter()
        .map(|finding| {
            let validated = validate_moment(input, finding)?;
            if !seen.insert(validated.semantic_fingerprint()) {
                return Err(DetectionValidationError::DuplicateFinding);
            }
            Ok(validated)
        })
        .collect()
}

fn validate_moment(
    input: &GameDetectionInput,
    mut finding: DetectedMoment,
) -> Result<ValidatedMoment, DetectionValidationError> {
    use DetectionValidationError::*;

    let anchor = input
        .timeline
        .occurrences()
        .get(finding.occurrence)
        .ok_or(InvalidOccurrence)?;
    if finding.code.trim().is_empty() {
        return Err(InvalidCode);
    }
    if let Some(assessment) = finding.tactical.as_ref() {
        let valid = match assessment.kind {
            crate::TacticalKind::HangingMaterial => {
                crate::hanging_material::validate_tactical_assessment(
                    input,
                    finding.occurrence,
                    assessment,
                )
            }
            crate::TacticalKind::PersonalTactics => {
                let pass = PositionPass::new(input, finding.occurrence).ok_or(InvalidOccurrence)?;
                crate::personal_tactics::validate(&pass, assessment)
            }
            crate::TacticalKind::WinningMate => {
                let pass = PositionPass::new(input, finding.occurrence).ok_or(InvalidOccurrence)?;
                validate_winning_mate(&pass, assessment)
            }
        };
        if !valid {
            return Err(InvalidTacticalAssessment);
        }
    }
    let board = &anchor.board;
    finding
        .subjects
        .sort_unstable_by(|a, b| a.square.cmp(&b.square));
    let mut previous = None;
    for subject in &finding.subjects {
        let square: Square = subject.square.parse().map_err(|_| InvalidSubject)?;
        if previous == Some(square)
            || board.piece_on(square) != Some(subject.role.piece())
            || board.color_on(square) != Some(subject.color.board_color())
        {
            return Err(InvalidSubject);
        }
        previous = Some(square);
    }
    let candidate_san = finding
        .candidate_move
        .as_deref()
        .map(|uci| {
            let mv = legal_move(board, uci).map_err(|_| InvalidCandidateMove)?;
            Ok(display_san_move(board, mv).to_string())
        })
        .transpose()?;
    Ok(ValidatedMoment {
        occurrence: finding.occurrence,
        code: finding.code,
        subjects: finding.subjects,
        candidate_move: finding.candidate_move.map(|uci| ChessAnswer { uci }),
        candidate_san,
        explanation: finding.explanation,
        fen: anchor.fen().into(),
        position: anchor.position().clone(),
        side_to_move: anchor.side_to_move(),
        opponent_lead_in: if finding.tactical.as_ref().is_some_and(|result| {
            result.puzzle.is_some()
                || result
                    .personal
                    .as_ref()
                    .is_some_and(|evidence| evidence.puzzle.is_some())
        }) {
            finding.occurrence.checked_sub(1).and_then(|ply| {
                let previous = &input.timeline.occurrences()[ply];
                crate::OpponentLeadIn::verified(
                    previous.fen(),
                    &previous.next_move()?.uci,
                    anchor.fen(),
                )
            })
        } else {
            None
        },
        tactical: finding.tactical,
    })
}

fn color_code(color: LearnerColor) -> u8 {
    match color {
        LearnerColor::White => 0,
        LearnerColor::Black => 1,
    }
}

fn hash_text(hash: &mut blake3::Hasher, value: &str) {
    hash.update(&(value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}

fn hash_candidate(hash: &mut blake3::Hasher, candidate: Option<&ChessAnswer>) {
    hash.update(&[u8::from(candidate.is_some())]);
    if let Some(candidate) = candidate {
        hash_text(hash, &candidate.uci);
    }
}
