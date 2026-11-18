//! Optional PGN context for teaching a prescribed repertoire response.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireLesson {
    /// The response in standard algebraic notation.
    pub san: String,
    /// Plain text PGN comments attached to this response.
    pub comments: Vec<String>,
    /// The previous move in a route through the position, in standard UCI.
    pub previous_move: Option<String>,
}
