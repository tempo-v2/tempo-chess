//! Pattern evidence, engine interpretation and only-move admission for played turns.
use crate::hanging_material::captures_on_board;
use crate::standard_position_canonicalizer::parse_position;
use crate::tactical_evaluation::{
    accepted_roots, checked_evaluation, complete_evaluation, score_order,
};
use crate::{
    BoardSubject, ChessAnswer, DetectedMoment, DetectionControl, DetectionOutcome, DetectionStop,
    DetectorDescriptor, EvaluationScore, EvaluationStatus, GameDetectionInput, GameDetector,
    LearnerColor, PieceRole, PositionPass, TacticalAssessment, TacticalEvaluationRequest,
    TacticalEvaluator, TacticalKind, TacticalOutcome,
};
use cozy_chess::{
    BitBoard, Board, Color, GameStatus, Move, Piece, Square, get_bishop_moves, get_king_moves,
    get_knight_moves, get_pawn_attacks, get_rook_moves, util::display_uci_move,
};
use serde::{Deserialize, Serialize};

const REPORT_BAND_CP: i32 = 30;
const ONLY_MOVE_GAP_CP: i64 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Motif {
    HangingMaterial,
    Fork,
    Pin,
    Skewer,
    DiscoveredAttack,
    DiscoveredCheck,
    MateInOne,
    MateInTwo,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct MotifCandidate {
    pub motif: Motif,
    pub answer: ChessAnswer,
    pub attackers: Vec<BoardSubject>,
    pub targets: Vec<BoardSubject>,
    /// One verified defense and mating finish for a mate in two.
    pub continuation: Vec<ChessAnswer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PersonalPuzzle {
    pub fen: String,
    pub line: Vec<ChessAnswer>,
    pub kind: PersonalPuzzleKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalPuzzleKind {
    OnlyMove,
    ForcedMate,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct PersonalTacticsEvidence {
    pub candidates: Vec<MotifCandidate>,
    pub puzzle: Option<PersonalPuzzle>,
}

pub struct PersonalTacticsDetector<E>(pub E);

impl<E: TacticalEvaluator> GameDetector for PersonalTacticsDetector<E> {
    fn descriptor(&self) -> DetectorDescriptor {
        let profile = self.0.descriptor();
        DetectorDescriptor {
            id: "personal-tactics".into(),
            revision: "3".into(),
            configuration_fingerprint: blake3::hash(
                &serde_json::to_vec(&(profile, REPORT_BAND_CP, ONLY_MOVE_GAP_CP))
                    .expect("serializable evaluator profile"),
            )
            .to_hex()
            .to_string(),
            label: "Personal tactics".into(),
        }
    }

    fn produces_tactical_assessments(&self) -> bool {
        true
    }

    fn tactical_evidence_format(&self) -> Option<crate::TacticalEvidenceFormat> {
        Some(crate::TacticalEvidenceFormat::CompleteDecision)
    }

    fn detect(
        &self,
        input: &GameDetectionInput,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        let mut findings = Vec::new();
        for occurrence in input.timeline().occurrences() {
            let pass = PositionPass::new(input, occurrence.offset()).expect("timeline occurrence");
            match self.detect_position(&pass, control) {
                DetectionOutcome::Complete(moments) => findings.extend(moments),
                other => return other,
            }
        }
        DetectionOutcome::Complete(findings)
    }

    fn detect_position(
        &self,
        pass: &PositionPass<'_>,
        control: &dyn DetectionControl,
    ) -> DetectionOutcome {
        match self.analyze(pass, control) {
            Ok(moment) => DetectionOutcome::Complete(moment.into_iter().collect()),
            Err(stop) => DetectionOutcome::Stopped(stop),
        }
    }
}

impl<E: TacticalEvaluator> PersonalTacticsDetector<E> {
    fn analyze(
        &self,
        pass: &PositionPass<'_>,
        control: &dyn DetectionControl,
    ) -> Result<Option<DetectedMoment>, DetectionStop> {
        let occurrence = pass.occurrence();
        let Some(played) = occurrence
            .next_move()
            .filter(|_| occurrence.side_to_move() == pass.learner())
        else {
            return Ok(None);
        };
        let candidates = candidates(pass, || control.check())?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let analysis = pass.compare(&self.0, control)?;
        let fen = format!("{} 0 1", occurrence.position().0);
        let standalone = if classification(&candidates, played, &analysis)
            == TacticalOutcome::MissedTactic
            && only_move(&candidates, &analysis).is_some()
        {
            Some(checked_evaluation(
                &self.0,
                &standalone_request(pass, &fen),
                control,
            )?)
        } else {
            None
        };
        let assessment = assemble(candidates, played, analysis, standalone, fen);
        Ok(Some(DetectedMoment {
            occurrence: occurrence.offset(),
            code: "personal_tactics".into(),
            subjects: vec![],
            candidate_move: assessment
                .personal
                .as_ref()
                .and_then(|e| e.candidates.first())
                .map(|c| c.answer.uci.clone()),
            explanation: "Tactical opportunities from the recorded game.".into(),
            tactical: Some(assessment),
        }))
    }
}

fn standalone_request(pass: &PositionPass<'_>, fen: &str) -> TacticalEvaluationRequest {
    TacticalEvaluationRequest {
        start_fen: fen.into(),
        fen: fen.into(),
        moves: vec![],
        legal_moves: pass
            .legal_moves()
            .iter()
            .map(|answer| answer.uci.clone())
            .collect(),
    }
}

pub(crate) fn validate(pass: &PositionPass<'_>, assessment: &TacticalAssessment) -> bool {
    let Some(played) = pass.occurrence().next_move() else {
        return false;
    };
    if pass.occurrence().side_to_move() != pass.learner() || &assessment.played != played {
        return false;
    }
    let Ok(candidates) = candidates(pass, || Ok(())) else {
        return false;
    };
    if candidates.is_empty() {
        return false;
    }
    if let EvaluationStatus::Complete { evaluation } = &assessment.analysis {
        if !complete_evaluation(&pass.engine_request(), evaluation, &evaluation.provenance) {
            return false;
        }
    }
    let fen = format!("{} 0 1", pass.occurrence().position().0);
    let needs_standalone = classification(&candidates, played, &assessment.analysis)
        == TacticalOutcome::MissedTactic
        && only_move(&candidates, &assessment.analysis).is_some();
    if needs_standalone != assessment.standalone_analysis.is_some() {
        return false;
    }
    if let Some(EvaluationStatus::Complete { evaluation }) = &assessment.standalone_analysis {
        let EvaluationStatus::Complete { evaluation: game } = &assessment.analysis else {
            return false;
        };
        if !complete_evaluation(
            &standalone_request(pass, &fen),
            evaluation,
            &game.provenance,
        ) {
            return false;
        }
    }
    *assessment
        == assemble(
            candidates,
            played,
            assessment.analysis.clone(),
            assessment.standalone_analysis.clone(),
            fen,
        )
}

impl TacticalAssessment {
    pub fn best_moves(&self) -> Vec<ChessAnswer> {
        accepted_roots(&self.analysis, 0)
    }
}

fn is_mate(motif: Motif) -> bool {
    matches!(motif, Motif::MateInOne | Motif::MateInTwo)
}

fn classification(
    candidates: &[MotifCandidate],
    played: &ChessAnswer,
    scores: &EvaluationStatus,
) -> TacticalOutcome {
    let EvaluationStatus::Complete { evaluation } = scores else {
        return TacticalOutcome::Uncertain;
    };
    let played_score = evaluation
        .roots
        .iter()
        .find(|row| row.uci == played.uci)
        .map(|row| row.score);
    let verified_mates: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| {
            let distance = match candidate.motif {
                Motif::MateInOne => 1,
                Motif::MateInTwo => 2,
                _ => return None,
            };
            (distance == 1
                || evaluation.roots.iter().any(|root| {
                    root.uci == candidate.answer.uci
                        && root.score == EvaluationScore::Mate(distance)
                }))
            .then_some((distance, &candidate.answer))
        })
        .collect();
    if let Some(fastest) = verified_mates.iter().map(|(distance, _)| *distance).min() {
        if verified_mates
            .iter()
            .any(|(distance, answer)| *distance == fastest && *answer == played)
        {
            return TacticalOutcome::PlayedTactic;
        }
        return if matches!(played_score, Some(EvaluationScore::Mate(n)) if n > 0) {
            TacticalOutcome::GoodAlternative
        } else {
            TacticalOutcome::MissedTactic
        };
    }
    if candidates.iter().any(|candidate| is_mate(candidate.motif)) {
        return TacticalOutcome::Uncertain;
    }
    let accepted = accepted_roots(scores, REPORT_BAND_CP);
    let qualifying: std::collections::BTreeSet<_> = candidates
        .iter()
        .filter(|c| accepted.contains(&c.answer))
        .map(|c| &c.answer.uci)
        .collect();
    let best_score = evaluation
        .roots
        .iter()
        .map(|root| root.score)
        .max_by(|left, right| score_order(*left, *right));
    if qualifying.is_empty() {
        TacticalOutcome::InferiorTactic
    } else if matches!(best_score, Some(EvaluationScore::Mate(_))) {
        if qualifying.contains(&played.uci) {
            TacticalOutcome::PlayedTactic
        } else if accepted.contains(played)
            || matches!(played_score, Some(EvaluationScore::Mate(distance)) if distance > 0)
        {
            TacticalOutcome::GoodAlternative
        } else {
            TacticalOutcome::MissedTactic
        }
    } else if qualifying.len() > 1 {
        if accepted.contains(played) {
            TacticalOutcome::UnclearPlay
        } else {
            TacticalOutcome::UnclearMiss
        }
    } else if qualifying.contains(&played.uci) {
        TacticalOutcome::PlayedTactic
    } else if accepted.contains(played) {
        TacticalOutcome::GoodAlternative
    } else {
        TacticalOutcome::MissedTactic
    }
}

fn only_move<'a>(
    candidates: &'a [MotifCandidate],
    scores: &EvaluationStatus,
) -> Option<&'a MotifCandidate> {
    let EvaluationStatus::Complete { evaluation } = scores else {
        return None;
    };
    let mut roots: Vec<_> = evaluation.roots.iter().collect();
    roots.sort_by(|left, right| score_order(right.score, left.score));
    let [best, second, ..] = roots.as_slice() else {
        return None;
    };
    let motif = match (best.score, second.score) {
        (EvaluationScore::Mate(1), second) if !matches!(second, EvaluationScore::Mate(n) if n > 0) => {
            Some(Motif::MateInOne)
        }
        (EvaluationScore::Mate(2), second) if !matches!(second, EvaluationScore::Mate(n) if n > 0) => {
            Some(Motif::MateInTwo)
        }
        (EvaluationScore::Centipawns(b), EvaluationScore::Centipawns(s))
            if i64::from(b) - i64::from(s) >= ONLY_MOVE_GAP_CP =>
        {
            None
        }
        (EvaluationScore::Centipawns(_), EvaluationScore::Mate(n)) if n < 0 => None,
        _ => return None,
    };
    candidates.iter().find(|c| {
        c.answer.uci == best.uci && motif.map_or(!is_mate(c.motif), |kind| kind == c.motif)
    })
}

fn assemble(
    candidates: Vec<MotifCandidate>,
    played: &ChessAnswer,
    analysis: EvaluationStatus,
    standalone: Option<EvaluationStatus>,
    fen: String,
) -> TacticalAssessment {
    let outcome = classification(&candidates, played, &analysis);
    let puzzle = standalone.as_ref().and_then(|scores| {
        let candidate = only_move(&candidates, &analysis)?;
        let retained = only_move(&candidates, scores)?;
        if outcome != TacticalOutcome::MissedTactic
            || candidate.answer != retained.answer
            || candidate.motif != retained.motif
        {
            return None;
        }
        let mut line = vec![candidate.answer.clone()];
        line.extend(candidate.continuation.clone());
        if is_mate(candidate.motif) {
            let board = parse_position(&fen, false).ok()?;
            let mv = crate::standard_uci::legal_move(&board, &candidate.answer.uci).ok()?;
            let proven = prove_mate(&board, mv, &[], &mut || Ok(())).ok()??;
            if proven.len() != candidate.continuation.len() {
                return None;
            }
            line.truncate(1);
            line.extend(proven);
        }
        Some(PersonalPuzzle {
            fen,
            line,
            kind: if is_mate(candidate.motif) {
                PersonalPuzzleKind::ForcedMate
            } else {
                PersonalPuzzleKind::OnlyMove
            },
        })
    });
    TacticalAssessment {
        kind: TacticalKind::PersonalTactics,
        candidates: vec![],
        played: played.clone(),
        outcome,
        accepted_moves: accepted_roots(&analysis, REPORT_BAND_CP),
        analysis,
        standalone_analysis: standalone,
        puzzle: None,
        equivalence_cp: REPORT_BAND_CP,
        personal: Some(PersonalTacticsEvidence { candidates, puzzle }),
    }
}

fn subject(board: &Board, square: Square) -> BoardSubject {
    BoardSubject {
        square: square.to_string(),
        role: PieceRole::from_piece(board.piece_on(square).expect("occupied square")),
        color: LearnerColor::from_board(board.color_on(square).expect("occupied square")),
    }
}

fn attacks(piece: Piece, from: Square, color: Color, occupied: BitBoard) -> BitBoard {
    match piece {
        Piece::King => get_king_moves(from),
        Piece::Queen => get_rook_moves(from, occupied) | get_bishop_moves(from, occupied),
        Piece::Rook => get_rook_moves(from, occupied),
        Piece::Bishop => get_bishop_moves(from, occupied),
        Piece::Knight => get_knight_moves(from),
        Piece::Pawn => get_pawn_attacks(from, color),
    }
}

fn slider(piece: Piece) -> bool {
    matches!(piece, Piece::Queen | Piece::Rook | Piece::Bishop)
}

fn moves(board: &Board) -> Vec<Move> {
    let mut moves = Vec::new();
    board.generate_moves(|group| {
        moves.extend(group);
        false
    });
    moves.sort_by_key(|mv| display_uci_move(board, *mv).to_string());
    moves
}

fn mate_moves(board: &Board) -> Vec<ChessAnswer> {
    if board.status() != GameStatus::Ongoing {
        return vec![];
    }
    let mut mates = Vec::new();
    board.generate_moves(|group| {
        for mv in group {
            let mut after = board.clone();
            after.play_unchecked(mv);
            if !after.checkers().is_empty() && after.status() == GameStatus::Won {
                mates.push(answer(board, mv));
            }
        }
        false
    });
    mates.sort_by(|a, b| a.uci.cmp(&b.uci));
    mates
}

fn repeated(board: &Board, history: &[&Board]) -> bool {
    history
        .iter()
        .filter(|prior| board.same_position(prior))
        .take(2)
        .count()
        >= 2
}

fn prove_mate(
    board: &Board,
    mv: Move,
    history: &[&Board],
    check: &mut impl FnMut() -> Result<(), DetectionStop>,
) -> Result<Option<Vec<ChessAnswer>>, DetectionStop> {
    let mut after = board.clone();
    after.play_unchecked(mv);
    match after.status() {
        GameStatus::Won => return Ok(Some(vec![])),
        GameStatus::Drawn => return Ok(None),
        GameStatus::Ongoing => {}
    }
    if repeated(&after, history) {
        return Ok(None);
    }
    let mut best_defense: Option<(usize, Vec<ChessAnswer>)> = None;
    for reply in moves(&after) {
        check()?;
        let mut reply_board = after.clone();
        reply_board.play_unchecked(reply);
        if repeated(&reply_board, history) {
            return Ok(None);
        }
        let mates = mate_moves(&reply_board);
        let Some(first) = mates.first() else {
            return Ok(None);
        };
        if best_defense
            .as_ref()
            .is_none_or(|(count, _)| mates.len() < *count)
        {
            best_defense = Some((mates.len(), vec![answer(&after, reply), first.clone()]));
        }
    }
    Ok(best_defense.map(|(_, line)| line))
}

fn answer(board: &Board, mv: Move) -> ChessAnswer {
    ChessAnswer {
        uci: display_uci_move(board, mv).to_string(),
    }
}

// Ordered by the tactical relationship, independent of engine scores and wire tags.
fn piece_rank(piece: Piece) -> u8 {
    match piece {
        Piece::Pawn => 0,
        Piece::Knight => 1,
        Piece::Bishop => 2,
        Piece::Rook => 3,
        Piece::Queen => 4,
        Piece::King => 5,
    }
}

fn ray_pairs(
    board: &Board,
    from: Square,
    piece: Piece,
    own: Color,
) -> Vec<(Motif, Square, Square)> {
    let occupied = board.occupied();
    let enemy = board.colors(!own);
    let mut pairs = Vec::new();
    for near in attacks(piece, from, own, occupied) & enemy {
        for far in attacks(piece, from, own, occupied ^ near.bitboard()) & enemy {
            if !cozy_chess::get_between_rays(from, far).has(near) {
                continue;
            }
            let near_role = piece_rank(board.piece_on(near).expect("front target"));
            let far_role = piece_rank(board.piece_on(far).expect("rear target"));
            if near_role < far_role {
                pairs.push((Motif::Pin, near, far));
            }
            if near_role > far_role {
                pairs.push((Motif::Skewer, near, far));
            }
        }
    }
    pairs
}

fn absolute_pins(board: &Board, color: Color) -> Vec<(Square, Square)> {
    let mut pins = vec![];
    for square in board.colors(!color) {
        let piece = board.piece_on(square).expect("occupied square");
        if !slider(piece) {
            continue;
        }
        for (kind, front, back) in ray_pairs(board, square, piece, !color) {
            if kind == Motif::Pin && back == board.king(color) {
                pins.push((front, square));
            }
        }
    }
    pins
}

fn legal_attack_squares(
    board: &Board,
    from: Square,
    color: Color,
    pins: &[(Square, Square)],
) -> BitBoard {
    let role = board
        .piece_on(from)
        .expect("attack originates on an occupied square");
    let targets = attacks(role, from, color, board.occupied());
    if let Some((_, pinner)) = pins.iter().find(|(square, _)| *square == from) {
        targets & cozy_chess::get_line_rays(board.king(color), *pinner)
    } else {
        targets
    }
}

fn candidates(
    pass: &PositionPass<'_>,
    mut check: impl FnMut() -> Result<(), DetectionStop>,
) -> Result<Vec<MotifCandidate>, DetectionStop> {
    let board = pass.board;
    let own = board.side_to_move();
    let history: Vec<_> = pass
        .earlier_boards()
        .take(board.halfmove_clock() as usize + 2)
        .chain([board])
        .collect();
    let mut results = Vec::new();
    let pinned = absolute_pins(board, !own);
    let own_pins = absolute_pins(board, own);
    for capture in captures_on_board(board, &pass.legal) {
        let mv =
            crate::standard_uci::legal_move(board, &capture.answer.uci).expect("legal capture");
        results.push(MotifCandidate {
            motif: Motif::HangingMaterial,
            answer: capture.answer,
            attackers: vec![subject(board, mv.from)],
            targets: vec![capture.target],
            continuation: vec![],
        });
    }
    for &mv in &pass.legal {
        check()?;
        let mut after = board.clone();
        after.play_unchecked(mv);
        let root = answer(board, mv);
        if board.color_on(mv.to) == Some(!own) {
            for (pinned_square, pinner) in &pinned {
                let piece = board.piece_on(*pinned_square).expect("pinned piece");
                if !attacks(piece, *pinned_square, !own, board.occupied()).has(mv.to)
                    || !after.pinned().has(*pinned_square)
                    || cozy_chess::get_line_rays(after.king(!own), *pinned_square).has(mv.to)
                    || after.checkers().len() > 1
                    || after.checkers().len() == 1 && !after.checkers().has(mv.to)
                {
                    continue;
                }
                results.push(MotifCandidate {
                    motif: Motif::Pin,
                    answer: root.clone(),
                    attackers: vec![subject(board, *pinner), subject(board, mv.from)],
                    targets: vec![subject(board, mv.to), subject(board, *pinned_square)],
                    continuation: vec![],
                });
            }
        }
        let enemy = after.colors(!own);
        let king = after.king(!own);
        let vacated = board.occupied() & !after.occupied();
        let next_pins = absolute_pins(&after, own);
        let mut changed_attacks = Vec::new();
        for to in after.colors(own) {
            let piece = after.piece_on(to).unwrap();
            let old = if board.color_on(to) == Some(own) && board.piece_on(to) == Some(piece) {
                to
            } else if mv.to == to || piece == Piece::King {
                mv.from
            } else if piece == Piece::Rook && board.color_on(mv.to) == Some(own) {
                mv.to
            } else {
                continue;
            };
            let previous = legal_attack_squares(board, old, own, &own_pins);
            let current = legal_attack_squares(&after, to, own, &next_pins) & enemy;
            let current = if after.checkers().has(to) {
                current
            } else {
                current ^ (current & king.bitboard())
            };
            let new = current & !previous;
            let mut push = |motif, targets: Vec<Square>| {
                results.push(MotifCandidate {
                    motif,
                    answer: root.clone(),
                    attackers: vec![subject(&after, to)],
                    targets: targets.into_iter().map(|sq| subject(&after, sq)).collect(),
                    continuation: vec![],
                });
            };
            if old != to && !new.is_empty() && current.len() >= 2 {
                push(Motif::Fork, current.into_iter().collect());
            }
            if old == to && slider(piece) {
                for target in new {
                    if (cozy_chess::get_between_rays(to, target) & vacated).is_empty() {
                        continue;
                    }
                    push(
                        if target == king {
                            Motif::DiscoveredCheck
                        } else {
                            Motif::DiscoveredAttack
                        },
                        vec![target],
                    );
                }
            }
            changed_attacks.extend(new.into_iter().map(|target| (to, target)));
            if slider(piece) {
                let old_pairs = if slider(board.piece_on(old).unwrap()) {
                    ray_pairs(board, old, board.piece_on(old).unwrap(), own)
                } else {
                    vec![]
                };
                for (motif, front, back) in ray_pairs(&after, to, piece, own) {
                    if !old_pairs.contains(&(motif, front, back)) {
                        push(motif, vec![front, back]);
                    }
                }
            }
        }
        // Two newly created attacks by distinct pieces can also be a double attack.
        if let Some((first, second)) =
            changed_attacks
                .iter()
                .enumerate()
                .find_map(|(index, first)| {
                    changed_attacks[index + 1..]
                        .iter()
                        .find(|second| first.0 != second.0 && first.1 != second.1)
                        .map(|second| (first, second))
                })
        {
            results.push(MotifCandidate {
                motif: Motif::Fork,
                answer: root.clone(),
                attackers: vec![subject(&after, first.0), subject(&after, second.0)],
                targets: vec![subject(&after, first.1), subject(&after, second.1)],
                continuation: vec![],
            });
        }
        if let Some(continuation) = prove_mate(board, mv, &history, &mut check)? {
            let from = mv.from;
            results.push(MotifCandidate {
                motif: if continuation.is_empty() {
                    Motif::MateInOne
                } else {
                    Motif::MateInTwo
                },
                answer: root,
                attackers: vec![subject(board, from)],
                targets: vec![subject(&after, king)],
                continuation,
            });
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Catalog, EvaluationFailure, EvaluationProvenance, LegalSequenceGrader, RootEvaluation,
        RootScore, ScoreBound, game_report_facts, plan_personal_puzzle, project_position,
        validate_detected_moments,
    };
    use tempo_study::Grader;

    struct Uninterrupted;
    impl DetectionControl for Uninterrupted {
        fn check(&self) -> Result<(), DetectionStop> {
            Ok(())
        }
    }

    struct Grades {
        best: &'static str,
        gap: i32,
        mate: bool,
    }
    impl TacticalEvaluator for Grades {
        fn descriptor(&self) -> EvaluationProvenance {
            EvaluationProvenance {
                engine: "test".into(),
                engine_version: "1".into(),
                configuration_fingerprint: "complete".into(),
            }
        }
        fn evaluate(
            &self,
            request: &TacticalEvaluationRequest,
            _: &dyn DetectionControl,
        ) -> Result<RootEvaluation, EvaluationFailure> {
            Ok(RootEvaluation {
                provenance: self.descriptor(),
                depth: 12,
                nodes: 10,
                roots: request
                    .legal_moves
                    .iter()
                    .map(|uci| RootScore {
                        uci: uci.clone(),
                        bound: ScoreBound::Exact,
                        score: if uci == self.best {
                            if self.mate {
                                EvaluationScore::Mate(2)
                            } else {
                                EvaluationScore::Centipawns(100)
                            }
                        } else if ["e5f3", "f7g8", "g1g2"].contains(&uci.as_str()) {
                            EvaluationScore::Centipawns(0)
                        } else {
                            EvaluationScore::Centipawns(100 - self.gap)
                        },
                    })
                    .collect(),
            })
        }
    }

    fn input(fen: &str, played: &str, name: &str) -> GameDetectionInput {
        let pgn = format!("[White \"{name}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n1. {played} 0-1");
        let game = crate::parse_played_games(&pgn)
            .unwrap()
            .pop()
            .unwrap()
            .unwrap();
        GameDetectionInput::new(game.timeline(), LearnerColor::White)
    }

    fn detection(input: &GameDetectionInput, engine: Grades) -> TacticalAssessment {
        let detector = PersonalTacticsDetector(engine);
        let DetectionOutcome::Complete(claims) = detector.detect(input, &Uninterrupted) else {
            panic!("detector must settle");
        };
        let validated = validate_detected_moments(input, claims).unwrap();
        validated.into_iter().next().unwrap().tactical.unwrap()
    }

    #[test]
    fn lifting_a_pin_without_vacating_the_attack_ray_is_not_a_discovery() {
        let fen = "4r2k/8/8/1q6/8/8/4B3/4K3 w - - 0 1";
        let game = input(fen, "Kd1", "fixture");
        let pass = PositionPass::new(&game, 0).unwrap();
        let discovered = candidates(&pass, || Ok(())).unwrap();
        assert!(!discovered.iter().any(|candidate| {
            candidate.answer.uci == "e1d1"
                && matches!(
                    candidate.motif,
                    Motif::DiscoveredAttack | Motif::DiscoveredCheck
                )
        }));
    }

    #[test]
    fn longer_forced_mates_are_comparable_without_a_centipawn_ambiguity() {
        struct LongerMates;
        impl TacticalEvaluator for LongerMates {
            fn descriptor(&self) -> EvaluationProvenance {
                Grades {
                    best: "d4e5",
                    gap: 75,
                    mate: false,
                }
                .descriptor()
            }
            fn evaluate(
                &self,
                request: &TacticalEvaluationRequest,
                _: &dyn DetectionControl,
            ) -> Result<RootEvaluation, EvaluationFailure> {
                Ok(RootEvaluation {
                    provenance: self.descriptor(),
                    depth: 12,
                    nodes: 10,
                    roots: request
                        .legal_moves
                        .iter()
                        .map(|uci| RootScore {
                            uci: uci.clone(),
                            bound: ScoreBound::Exact,
                            score: match uci.as_str() {
                                "d4e5" | "a1a7" => EvaluationScore::Mate(3),
                                "g1g2" => EvaluationScore::Mate(5),
                                _ => EvaluationScore::Centipawns(0),
                            },
                        })
                        .collect(),
                })
            }
        }
        let fen = "7k/n7/8/4q3/3P4/8/8/R5K1 w - - 0 1";
        for (played, outcome) in [
            ("Kg2", TacticalOutcome::GoodAlternative),
            ("dxe5", TacticalOutcome::PlayedTactic),
            ("Kh1", TacticalOutcome::MissedTactic),
        ] {
            let input = input(fen, played, "fixture");
            let detector = PersonalTacticsDetector(LongerMates);
            let DetectionOutcome::Complete(claims) = detector.detect(&input, &Uninterrupted) else {
                panic!("fixture should be a complete position read");
            };
            let mut findings = validate_detected_moments(&input, claims).unwrap();
            let tactical = findings.pop().unwrap().tactical.unwrap();
            assert_eq!(tactical.outcome, outcome);
            let personal = tactical.personal.unwrap();
            assert!(
                personal
                    .candidates
                    .iter()
                    .all(|candidate| !is_mate(candidate.motif))
            );
            assert!(personal.puzzle.is_none());
        }
    }

    #[test]
    fn fork_reporting_and_only_move_admission_are_independent_of_names() {
        let fen = "1k6/4q3/8/4N3/8/8/8/5K2 w - - 0 1";
        let mut accepted = None;
        for (gap, admitted) in [(74, false), (75, true)] {
            for name in ["Andy", "Changed", ""] {
                let input = input(fen, "Nf3", name);
                let result = detection(
                    &input,
                    Grades {
                        best: "e5c6",
                        gap,
                        mate: false,
                    },
                );
                assert_eq!(result.outcome, TacticalOutcome::MissedTactic);
                let evidence = result.personal.unwrap();
                assert_eq!(evidence.puzzle.is_some(), admitted);
                assert!(
                    evidence
                        .candidates
                        .iter()
                        .any(|candidate| candidate.motif == Motif::Fork
                            && candidate.answer.uci == "e5c6"
                            && candidate.attackers[0].role == PieceRole::Knight
                            && candidate.targets.len() == 2)
                );
                if admitted {
                    if let Some(previous) = &accepted {
                        assert_eq!(previous, &evidence);
                    }
                    accepted = Some(evidence);
                }
            }
        }
    }

    #[test]
    fn the_reporting_band_includes_thirty_and_preserves_ambiguous_misses() {
        let game = input("7k/n7/8/4q3/3P4/8/8/R5K1 w - - 0 1", "Kg2", "Learner");
        for (gap, expected) in [
            (30, TacticalOutcome::UnclearMiss),
            (31, TacticalOutcome::MissedTactic),
        ] {
            let assessed = detection(
                &game,
                Grades {
                    best: "d4e5",
                    gap,
                    mate: false,
                },
            );
            assert_eq!(assessed.outcome, expected);
            assert!(assessed.personal.unwrap().puzzle.is_none());
        }
    }

    #[test]
    fn a_mate_in_two_accepts_any_legal_mating_finish_and_replays_that_finish() {
        let fen = "8/5Q2/k2K4/8/8/8/8/8 w - - 0 1";
        let input = input(fen, "Qg8", "Andy");
        let result = detection(
            &input,
            Grades {
                best: "d6c5",
                gap: 100,
                mate: true,
            },
        );
        assert_eq!(result.outcome, TacticalOutcome::MissedTactic);
        let puzzle = result.personal.unwrap().puzzle.unwrap();
        assert_eq!(puzzle.kind, PersonalPuzzleKind::ForcedMate);
        assert_eq!(
            puzzle
                .line
                .iter()
                .map(|move_| move_.uci.as_str())
                .collect::<Vec<_>>(),
            ["d6c5", "a6a5", "f7a2"]
        );
        let plan = plan_personal_puzzle(&Catalog::default(), &puzzle, &[]).unwrap();
        let definition = &plan.content.catalog.items[&plan.target].definition;
        let start = LegalSequenceGrader.start(definition).unwrap();
        let next = LegalSequenceGrader
            .answer(definition, &start.state, &"d6c5".into())
            .unwrap();
        assert_eq!(next.feedback.autoplay, vec![ChessAnswer::from("a6a5")]);
        assert!(!next.complete);
        let finished = LegalSequenceGrader
            .answer(definition, &next.state, &"f7a7".into())
            .unwrap();
        assert!(finished.complete);
        let serialized = serde_json::to_string(&finished.state).unwrap();
        let recovered = serde_json::from_str(&serialized).unwrap();
        let last = project_position(definition, &recovered, false).unwrap();
        assert!(last.display_fen.starts_with("8/Q7/"));
        assert_eq!(
            crate::revealed_attempt_line(definition, &recovered)
                .unwrap()
                .moves
                .last()
                .unwrap()
                .fen,
            last.display_fen
        );
        assert!(
            serde_json::from_str::<crate::ChessState>(r#"{"decision_index":1}"#)
                .unwrap()
                .played_mate
                .is_none()
        );
    }

    #[test]
    fn all_piece_roles_can_fork_and_absolute_pins_restrict_threats() {
        for (fen, san, uci, role) in [
            (
                "7k/8/2q1r3/8/3P4/8/8/K7 w - - 0 1",
                "d5",
                "d4d5",
                PieceRole::Pawn,
            ),
            (
                "1k6/4q3/8/4N3/8/8/8/5K2 w - - 0 1",
                "Nc6+",
                "e5c6",
                PieceRole::Knight,
            ),
            (
                "7k/8/8/1q3r2/8/8/4B3/K7 w - - 0 1",
                "Bd3",
                "e2d3",
                PieceRole::Bishop,
            ),
            (
                "7k/8/8/8/2q3r1/8/8/3R3K w - - 0 1",
                "Rd4",
                "d1d4",
                PieceRole::Rook,
            ),
            (
                "7k/8/8/8/2b5/r7/4Q3/7K w - - 0 1",
                "Qd3",
                "e2d3",
                PieceRole::Queen,
            ),
            (
                "7k/8/8/2n1n3/8/3K4/8/8 w - - 0 1",
                "Kd4",
                "d3d4",
                PieceRole::King,
            ),
        ] {
            let game = input(fen, san, "Learner");
            let pass = PositionPass::new(&game, 0).unwrap();
            assert!(
                candidates(&pass, || Ok(()))
                    .unwrap()
                    .iter()
                    .any(|c| c.motif == Motif::Fork
                        && c.answer.uci == uci
                        && c.attackers.len() == 1
                        && c.attackers[0].role == role),
                "{role:?}"
            );
        }
        let game = input("4r2k/8/8/1b4n1/4R3/8/8/4K3 w - - 0 1", "Re5", "Learner");
        let pass = PositionPass::new(&game, 0).unwrap();
        assert!(
            !candidates(&pass, || Ok(()))
                .unwrap()
                .iter()
                .any(|c| c.motif == Motif::Fork && c.answer.uci == "e4e5")
        );
    }

    #[test]
    fn pin_and_skewer_use_bishop_over_knight_and_discoveries_keep_the_revealed_piece() {
        for (fen, san, uci, motif) in [
            (
                "7k/8/3b4/8/3n4/8/8/R6K w - - 0 1",
                "Rd1",
                "a1d1",
                Motif::Pin,
            ),
            (
                "7k/8/3n4/8/3b4/8/8/R6K w - - 0 1",
                "Rd1",
                "a1d1",
                Motif::Skewer,
            ),
            (
                "7k/q7/8/8/8/8/N7/R6K w - - 0 1",
                "Nb4",
                "a2b4",
                Motif::DiscoveredAttack,
            ),
            (
                "k7/8/8/8/8/8/N7/R6K w - - 0 1",
                "Nb4+",
                "a2b4",
                Motif::DiscoveredCheck,
            ),
        ] {
            let game = input(fen, san, "Learner");
            let pass = PositionPass::new(&game, 0).unwrap();
            assert!(
                candidates(&pass, || Ok(()))
                    .unwrap()
                    .iter()
                    .any(|c| c.motif == motif
                        && c.answer.uci == uci
                        && c.attackers[0].role == PieceRole::Rook),
                "{motif:?}"
            );
        }
    }

    #[test]
    fn clocks_and_game_result_have_one_interpretation() {
        let learner = LearnerColor::Black;
        let facts = game_report_facts(
            "[TimeControl \"60\"]\n[WhiteElo \"1459\"]\n[BlackElo \"1500\"]\n\n1. e4 0-1",
            learner,
        );
        let same_clock = game_report_facts("[TimeControl \"60+0\"]\n\n1. e4 1-0", learner);
        assert_eq!(facts.time_control, same_clock.time_control);
        assert_eq!(facts.result, crate::LearnerGameResult::Win);
        assert_eq!(same_clock.result, crate::LearnerGameResult::Loss);
        assert_eq!(
            (facts.learner_rating, facts.opponent_rating),
            (Some(1500), Some(1459))
        );
    }
}
