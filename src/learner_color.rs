//! Which side supplies the exercises in a repertoire import.

use serde::{Deserialize, Serialize};

/// Choose which player's moves are learned when importing an orthodox PGN.
/// The other player's moves provide the positions for subsequent questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnerColor {
    White,
    Black,
}

impl LearnerColor {
    pub(crate) fn board_color(self) -> cozy_chess::Color {
        match self {
            Self::White => cozy_chess::Color::White,
            Self::Black => cozy_chess::Color::Black,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ContentParser, ImportRequest, LearnerColor, RepertoirePgnParser};

    #[test]
    fn black_studies_blacks_reply_to_the_first_move() {
        let catalog = RepertoirePgnParser::new("black", "Black openings")
            .parse(&ImportRequest {
                pgn: "1. e4 c5 *".into(),
                source: "Sicilian".into(),
                repertoire: Some("black".into()),
                learner_color: Some(LearnerColor::Black),
            })
            .unwrap();
        let target = &catalog.lines[0].targets[0];
        let decision = &catalog.items[target].definition.decisions[0];
        assert_eq!(decision.expected.uci, "c7c5");
        assert!(decision.fen.contains(" b KQkq "));
    }
}
