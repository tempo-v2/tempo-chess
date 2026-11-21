//! Consecutive learner decisions on legal authored paths.
use crate::parsed_pgn::{PgnGame, parse_pgn};
use crate::standard_position_canonicalizer::board_key;
use crate::{ChessError, ContentItem, ContentKind, OpponentLeadIn};
use std::collections::BTreeMap;
use tempo_types::{ContentVersion, TargetId};

/// One occurrence of a current repertoire answer, with its exact incoming ply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepertoireStudyStep {
    pub target: TargetId,
    pub version: ContentVersion,
    pub lead_in: Option<OpponentLeadIn>,
}

/// Join the saved repertoire document to its current answers. Every returned
/// path keeps the document's move order. An unavailable or changed answer cuts
/// the path so the caller cannot bridge over it by filtering the output.
pub fn repertoire_study_routes<'a>(
    pgn: &str,
    repertoire: &str,
    items: impl IntoIterator<Item = &'a ContentItem>,
) -> Result<Vec<Vec<RepertoireStudyStep>>, ChessError> {
    let targets: BTreeMap<_, _> = items
        .into_iter()
        .filter_map(|item| match &item.definition.kind {
            ContentKind::Repertoire {
                repertoire: owner,
                position,
            } if owner == repertoire && item.active() => Some((position, item)),
            _ => None,
        })
        .collect();
    let Some(item) = targets.values().next() else {
        return Ok(Vec::new());
    };
    let learner = item.definition.learner_color()?.board_color();
    let mut routes = Vec::new();
    for game in parse_pgn(pgn)? {
        for route in game.routes() {
            let mut segment = Vec::new();
            for index in route {
                let ply = &game.plies[index];
                if ply.board.side_to_move() != learner {
                    continue;
                }
                let step = targets
                    .get(&board_key(&ply.board))
                    .and_then(|item| study_step(&game, index, item));
                if let Some(step) = step {
                    segment.push(step);
                } else if !segment.is_empty() {
                    routes.push(std::mem::take(&mut segment));
                }
            }
            if !segment.is_empty() {
                routes.push(segment);
            }
        }
    }
    Ok(routes)
}

fn study_step(game: &PgnGame<'_>, index: usize, item: &ContentItem) -> Option<RepertoireStudyStep> {
    let [decision] = item.definition.decisions.as_slice() else {
        return None;
    };
    let ply = &game.plies[index];
    if decision.expected.uci != ply.uci
        || !decision.autoplay.is_empty()
        || !crate::standard_position_canonicalizer::parse_position(&decision.fen, true)
            .ok()
            .is_some_and(|board| board_key(&board) == board_key(&ply.board))
    {
        return None;
    }
    Some(RepertoireStudyStep {
        target: item.target.clone(),
        version: item.version,
        lead_in: OpponentLeadIn::from_pgn(game, index),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentStatus, LearnerColor, compile_repertoire};

    #[test]
    fn authored_branches_keep_their_replies_and_missing_answers_cut_the_path() {
        let pgn = "1. e4 c5 (1... e5 2. Nf3 Nc6 3. Bb5) 2. Nf3 d6 3. d4 *";
        let mut catalog = compile_repertoire(pgn, "r", "name", LearnerColor::White, "pgn").unwrap();
        let routes = repertoire_study_routes(pgn, "r", catalog.items.values()).unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0][0].target, routes[1][0].target);
        assert_eq!(routes[0][1].lead_in.as_ref().unwrap().uci, "c7c5");
        assert_eq!(routes[1][1].lead_in.as_ref().unwrap().uci, "e7e5");

        catalog.items.get_mut(&routes[0][1].target).unwrap().status = ContentStatus::Retired;
        let cut = repertoire_study_routes(pgn, "r", catalog.items.values()).unwrap();
        assert_eq!(cut[0], routes[0][..1]);
        assert_eq!(cut[1], routes[0][2..]);
        assert_eq!(cut[2], routes[1]);
    }
}
