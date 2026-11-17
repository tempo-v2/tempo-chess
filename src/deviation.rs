//! Deviation and its usage scenarios.

use serde::{Deserialize, Serialize};
use tempo_types::TargetId;

/// A fact about a played move that differs from the supplied repertoire.
///
/// # When to use
///
/// A [`crate::GameComparator`] produces these facts. The application can attach
/// provider/event identity and choose to submit an external learning assessment.
/// The comparator never changes learning itself.
///
/// `target` refers to current supplied content, `ply` locates the move in the game
/// (the fixture uses one-based plies), and `observed_move` records what was played.
/// Content version and dataset checks belong to the application command that acts
/// on the comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deviation {
    pub target: TargetId,
    pub ply: u32,
    pub observed_move: String,
}

#[cfg(test)]
mod tests {
    use crate::{GAME_PGN, GameComparator, demo_catalog, demo_comparator};
    use tempo_types::TargetId;

    #[test]
    fn game_feedback_identifies_the_target_and_the_observed_move() {
        let catalog = demo_catalog();
        let before = catalog.clone();
        let deviations = demo_comparator().compare(GAME_PGN, &catalog).unwrap();
        assert_eq!(deviations.len(), 1);
        let deviation = &deviations[0];
        assert_eq!(deviation.target, TargetId::from("r1:p1"));
        assert_eq!(deviation.ply, 1);
        assert_eq!(deviation.observed_move, "d2d4");
        assert_eq!(catalog, before); // The fact can now feed an application command.
    }
}
