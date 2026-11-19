//! Searchable opening facts from an already accepted game. This read stops after
//! twelve main-line plies and never interprets a provider's opening name as coverage.
use crate::pgn_tokens::{PgnTokens, Token};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PlayedOpening {
    pub opening: Option<String>,
    pub variation: Option<String>,
    pub eco: Option<String>,
    pub eco_url: Option<String>,
    pub moves: Vec<String>,
}

pub fn played_opening(pgn: &str) -> Result<PlayedOpening, crate::ChessError> {
    let mut tokens = PgnTokens::new(pgn);
    let mut opening = PlayedOpening::default();
    let mut variation_depth = 0_usize;
    while let Some(token) = tokens.next()? {
        match token {
            Token::Tag(tag, text) if opening.moves.is_empty() => match tag {
                "Opening" => opening.opening = Some(text.into()),
                "Variation" => opening.variation = Some(text.into()),
                "ECO" => opening.eco = Some(text.into()),
                "ECOUrl" => opening.eco_url = Some(text.into()),
                _ => {}
            },
            Token::VariationStart => variation_depth += 1,
            Token::VariationEnd => variation_depth = variation_depth.saturating_sub(1),
            Token::San(san) if variation_depth == 0 => {
                opening.moves.push(san.to_owned());
                if opening.moves.len() == 12 {
                    break;
                }
            }
            Token::Result(_) if variation_depth == 0 => break,
            _ => {}
        }
    }
    Ok(opening)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_labels_and_mainline_moves_are_independent() {
        let moves = "1.e4 {book} e5 (1... c5 (1... e6)) 2.Nf3 Nc6 3.Bc4 Bc5 4.d3 d6 5.Nc3 Nf6 6.O-O O-O 7.Bg5 1-0";
        let untagged = played_opening(moves).unwrap();
        assert_eq!(
            untagged.moves,
            [
                "e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "d3", "d6", "Nc3", "Nf6", "O-O", "O-O"
            ]
        );
        assert_eq!(untagged.opening, None);
        let tagged = played_opening(&format!("[Opening \"Italian Game\"]\n{moves}")).unwrap();
        assert_eq!(tagged.opening.as_deref(), Some("Italian Game"));
        assert_eq!(tagged.moves, untagged.moves);
    }
}
