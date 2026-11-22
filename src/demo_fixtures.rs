//! Small, explicit catalogs and registered inputs for application wiring.
//!
//! These legal opening positions exercise the interfaces. The multi-decision
//! fixture is not a claim that the position is a tactical puzzle. The public
//! helpers remain here because they describe one coherent demo dataset.

use crate::{
    Catalog, ChessDefinition, ContentItem, ContentKind, ContentStatus, Decision, Deviation,
    FixtureCanonicalizer, FixtureComparator, FixtureParser, GradingScheme, PositionKey,
    RepertoireLine,
};
use tempo_types::ContentVersion;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
pub const SECOND_FEN: &str = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2";
pub const REPERTOIRE_PGN: &str = "[Event \"Tempo repertoire fixture\"]\n\n1. e4 e5 2. Nf3 *";
pub const TACTIC_PGN: &str =
    "[Event \"Tempo multi-decision interface fixture\"]\n\n1. e4 e5 2. Nf3 *";
pub const GAME_PGN: &str = "[Event \"Tempo deviation fixture\"]\n\n1. d4 *";
pub fn demo_canonicalizer() -> FixtureCanonicalizer {
    FixtureCanonicalizer::default()
        .with_position(START_FEN, PositionKey("start".into()))
        .with_position(SECOND_FEN, PositionKey("after-e4-e5".into()))
}
pub fn demo_catalog() -> Catalog {
    let decision = Decision {
        fen: START_FEN.into(),
        expected: "e2e4".into(),
        autoplay: vec!["e7e5".into()],
    };
    let second = Decision {
        fen: SECOND_FEN.into(),
        expected: "g1f3".into(),
        autoplay: vec![],
    };
    let items = [
        ContentItem {
            target: "r1:p1".into(),
            version: ContentVersion(1),
            label: "Repertoire: first move".into(),
            status: ContentStatus::Active,
            definition: ChessDefinition {
                scheme: GradingScheme::ExactSequenceV1,
                kind: ContentKind::Repertoire {
                    repertoire: "r1".into(),
                    position: PositionKey("start".into()),
                },
                decisions: vec![decision.clone()],
            },
            provenance: vec!["fixture".into()],
            payoff_origin_version: None,
            verified_tactic: Vec::new(),
            game_capture: None,
            opponent_lead_in: crate::LeadInEvidence::Unavailable,
            lesson: None,
        },
        ContentItem {
            target: "r1:p2".into(),
            version: ContentVersion(1),
            label: "Repertoire: second move".into(),
            status: ContentStatus::Active,
            definition: ChessDefinition {
                scheme: GradingScheme::ExactSequenceV1,
                kind: ContentKind::Repertoire {
                    repertoire: "r1".into(),
                    position: PositionKey("after-e4-e5".into()),
                },
                decisions: vec![second.clone()],
            },
            provenance: vec!["fixture".into()],
            payoff_origin_version: None,
            verified_tactic: Vec::new(),
            game_capture: None,
            opponent_lead_in: crate::LeadInEvidence::Unavailable,
            lesson: None,
        },
        ContentItem {
            target: "t1".into(),
            version: ContentVersion(1),
            label: "Two-decision interface fixture (not a real tactic)".into(),
            status: ContentStatus::Active,
            definition: ChessDefinition {
                scheme: GradingScheme::ExactSequenceV1,
                kind: ContentKind::Tactic {
                    practice_start: PositionKey("start".into()),
                },
                decisions: vec![decision, second],
            },
            provenance: vec!["fixture".into()],
            payoff_origin_version: None,
            verified_tactic: Vec::new(),
            game_capture: None,
            opponent_lead_in: crate::LeadInEvidence::Unavailable,
            lesson: None,
        },
    ];
    Catalog {
        items: items
            .into_iter()
            .map(|item| (item.target.clone(), item))
            .collect(),
        lines: vec![RepertoireLine {
            id: "line-1".into(),
            repertoire: "r1".into(),
            targets: vec!["r1:p1".into(), "r1:p2".into()],
        }],
    }
}
pub fn demo_parser() -> FixtureParser {
    let all = demo_catalog();
    let repertoire = Catalog {
        items: all
            .items
            .iter()
            .filter(|(_, v)| v.definition.is_repertoire())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        lines: all.lines.clone(),
    };
    let tactic = Catalog {
        items: all
            .items
            .iter()
            .filter(|(_, v)| !v.definition.is_repertoire())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        lines: Vec::new(),
    };
    FixtureParser::default()
        .with_fixture(REPERTOIRE_PGN, repertoire)
        .with_fixture(TACTIC_PGN, tactic)
}
pub fn demo_comparator() -> FixtureComparator {
    FixtureComparator::default().with_fixture(
        GAME_PGN,
        vec![Deviation {
            target: "r1:p1".into(),
            ply: 1,
            observed_move: "d2d4".into(),
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentParser, GameComparator};

    #[test]
    fn demo_ports_agree_on_the_same_catalog_targets() {
        let catalog = demo_catalog();
        catalog.validate().unwrap();
        let parsed = demo_parser()
            .parse(&crate::ImportRequest {
                pgn: REPERTOIRE_PGN.into(),
                source: "demo".into(),
                repertoire: Some("r1".into()),
                learner_color: None,
            })
            .unwrap();
        assert!(
            parsed
                .items
                .keys()
                .all(|target| catalog.items.contains_key(target))
        );
        let deviations = demo_comparator().compare(GAME_PGN, &catalog).unwrap();
        assert!(
            deviations
                .iter()
                .all(|fact| parsed.items.contains_key(&fact.target))
        );
    }
}
