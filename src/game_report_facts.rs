//! Read authoritative facts from an already accepted, single-game PGN.
use crate::LearnerColor;
use crate::pgn_tokens::{PgnTokens, Token};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct GameTimeControl {
    pub seconds: u32,
    pub increment: u32,
}

impl GameTimeControl {
    pub fn from_tag(value: &str) -> Option<Self> {
        let (base, increment) = value.split_once('+').unwrap_or((value, "0"));
        let seconds = base.parse().ok()?;
        let increment = increment.parse().ok()?;
        (seconds > 0 || increment > 0).then_some(Self { seconds, increment })
    }

    pub fn estimated_seconds(self) -> u64 {
        u64::from(self.seconds) + 40 * u64::from(self.increment)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnerGameResult {
    Win,
    Draw,
    Loss,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Default)]
pub struct GameReportFacts {
    pub time_control: Option<GameTimeControl>,
    pub learner_rating: Option<u32>,
    pub opponent_rating: Option<u32>,
    pub result: LearnerGameResult,
}

pub fn game_report_facts(pgn: &str, learner: LearnerColor) -> GameReportFacts {
    let mut tokens = PgnTokens::new(pgn);
    let mut facts = GameReportFacts::default();
    let mut white_rating = None;
    let mut black_rating = None;
    let mut result = None;
    let mut depth = 0usize;
    while let Ok(Some(token)) = tokens.next() {
        match token {
            Token::Tag("TimeControl", value) => {
                facts.time_control = GameTimeControl::from_tag(value)
            }
            Token::Tag("WhiteElo", value) => white_rating = value.parse().ok(),
            Token::Tag("BlackElo", value) => black_rating = value.parse().ok(),
            Token::Tag("Result", value) => result = Some(value),
            Token::Result(value) if depth == 0 => {
                result = Some(value);
                break;
            }
            Token::VariationStart => depth += 1,
            Token::VariationEnd => depth = depth.saturating_sub(1),
            Token::San(_) | Token::MoveNumber { .. } if result.is_some() => break,
            _ => {}
        }
    }
    (facts.learner_rating, facts.opponent_rating) = match learner {
        LearnerColor::White => (white_rating, black_rating),
        LearnerColor::Black => (black_rating, white_rating),
    };
    facts.result = match (learner, result) {
        (_, Some("1/2-1/2")) => LearnerGameResult::Draw,
        (LearnerColor::White, Some("1-0")) | (LearnerColor::Black, Some("0-1")) => {
            LearnerGameResult::Win
        }
        (_, Some("1-0" | "0-1")) => LearnerGameResult::Loss,
        _ => LearnerGameResult::Unknown,
    };
    facts
}
