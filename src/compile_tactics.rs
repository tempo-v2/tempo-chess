//! Compile a fully normalized batch into a proposed Chess catalog.

use crate::standard_position_canonicalizer::{board_key, parse_position};
use crate::standard_uci::legal_move;
use crate::tactic_record::is_source_token;
use crate::{
    Catalog, ChessDefinition, ChessError, ContentItem, ContentKind, ContentStatus, Decision,
    GradingScheme, TacticRecord,
};
use std::collections::BTreeSet;
use tempo_types::ContentVersion;

/// Validate complete learner-first lines and compile them as decision chains.
/// Pass the resulting catalog to [`crate::plan_import`] for merges and quarantine.
pub fn compile_tactics(records: &[TacticRecord]) -> Result<Catalog, ChessError> {
    let mut seen = BTreeSet::new();
    for record in records {
        if record.namespace.trim().is_empty() || record.source_id.trim().is_empty() {
            return Err(ChessError::InvalidContent(
                "tactic namespace and source ID are required".into(),
            ));
        }
        if !seen.insert(record.target_id()) {
            return Err(invalid(record, "duplicate tactic identity"));
        }
    }
    let mut catalog = Catalog::default();
    for record in records {
        let item = compile_record(record)?;
        catalog.items.insert(item.target.clone(), item);
    }
    catalog.validate()?;
    Ok(catalog)
}

fn compile_record(record: &TacticRecord) -> Result<ContentItem, ChessError> {
    if record.solution.is_empty() {
        return Err(invalid(record, "missing learner move"));
    }
    if record.provenance.iter().any(|value| is_source_token(value)) {
        return Err(invalid(
            record,
            "provenance includes a reserved tactic source token",
        ));
    }
    let start = parse_position(&record.solver_fen, false).map_err(|e| invalid(record, e))?;
    let practice_start = board_key(&start);
    // Ignore clocks and ineffective en passant in the initial display so identical
    // board/solution pairs compile identically across source formats.
    let mut board = parse_position(&format!("{} 0 1", practice_start.0), false)?;
    let mut decisions = Vec::with_capacity(record.solution.len().div_ceil(2));
    for (index, pair) in record.solution.chunks(2).enumerate() {
        let mut decision = Decision {
            fen: board.to_string(),
            expected: pair[0].as_str().into(),
            autoplay: Vec::new(),
        };
        for (offset, uci) in pair.iter().enumerate() {
            let ply = index * 2 + offset + 1;
            let mv = legal_move(&board, uci)
                .map_err(|error| invalid(record, format!("ply {ply}: {error}")))?;
            board.play(mv);
        }
        decision
            .autoplay
            .extend(pair.get(1).map(|uci| uci.as_str().into()));
        decisions.push(decision);
    }
    let target = record.target_id();
    let mut provenance = record.provenance.clone();
    provenance.push(target.0.clone());
    provenance.sort();
    provenance.dedup();
    Ok(ContentItem {
        target,
        version: ContentVersion(1),
        label: record.label.clone(),
        status: ContentStatus::Active,
        definition: ChessDefinition {
            scheme: GradingScheme::ExactSequenceV1,
            kind: ContentKind::Tactic { practice_start },
            decisions,
        },
        provenance,
    })
}

fn invalid(record: &TacticRecord, message: impl std::fmt::Display) -> ChessError {
    ChessError::InvalidContent(format!(
        "tactic {:?}/{:?}: {message}",
        record.namespace, record.source_id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{START_FEN, apply_setup_move};

    fn record(source_id: &str, solver_fen: &str, solution: &[&str]) -> TacticRecord {
        TacticRecord {
            namespace: "test".into(),
            source_id: source_id.into(),
            solver_fen: solver_fen.into(),
            solution: solution.iter().map(|s| (*s).into()).collect(),
            label: source_id.into(),
            provenance: vec![],
        }
    }

    #[test]
    fn compiles_real_short_and_long_pack_rows_with_both_learner_colors() {
        // Legacy fork-focused-01, Lichess IDs 01iMq and 01o3H.
        let white = apply_setup_move(
            "5r2/p1q2nk1/b1p2r1p/3p2pQ/3N2P1/1B2R2P/PPP5/3R2K1 b - - 3 29",
            "f7d6",
        )
        .unwrap();
        let black = apply_setup_move(
            "r4rk1/BRp2pp1/3b3p/8/2P2P2/6P1/P6P/3R2K1 w - - 1 23",
            "c4c5",
        )
        .unwrap();
        let rows = [
            record("01iMq", &white, &["d4e6", "f6e6", "e3e6"]),
            record("01o3H", &black, &["a8a7", "b7a7", "d6c5", "g1f1", "c5a7"]),
        ];
        let catalog = compile_tactics(&rows).unwrap();
        for (row, turn) in rows.iter().zip(["w", "b"]) {
            let decisions = &catalog.items[&row.target_id()].definition.decisions;
            assert_eq!(decisions[0].fen.split_whitespace().nth(1), Some(turn));
            let compiled = decisions
                .iter()
                .flat_map(|d| {
                    std::iter::once(&d.expected.uci).chain(d.autoplay.iter().map(|a| &a.uci))
                })
                .collect::<Vec<_>>();
            assert_eq!(compiled, row.solution.iter().collect::<Vec<_>>());
        }
    }

    #[test]
    fn invalid_reply_and_duplicate_identity_reject_the_whole_batch() {
        let valid = record("a", START_FEN, &["e2e4"]);
        let illegal_reply = record("b", START_FEN, &["e2e4", "e7e4"]);
        assert!(
            compile_tactics(&[valid.clone(), illegal_reply])
                .unwrap_err()
                .to_string()
                .contains("ply 2")
        );
        assert!(
            compile_tactics(&[valid.clone(), valid])
                .unwrap_err()
                .to_string()
                .contains("duplicate tactic identity")
        );
        assert!(compile_tactics(&[record("c", START_FEN, &[])]).is_err());
        assert!(compile_tactics(&[record("c", "8/8/8/8/8/8/8/8 w - - 0 1", &["a2a3"])]).is_err());
    }
}
