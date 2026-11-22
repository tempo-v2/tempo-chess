//! Partition an authored Black PGN while retaining its movetext annotations.
use crate::{
    AuthoredRepertoire, ChessError,
    parsed_pgn::{invalid, parse_pgn},
    pgn_tokens::{PgnTokens, Token},
};
use std::collections::BTreeSet;

/// Return PGNs for 1.e4, 1.d4, and all other first moves, respectively.
/// A chapter wholly in one group is copied verbatim.
pub fn partition_black_openings(pgn: &str) -> Result<[String; 3], ChessError> {
    let original = AuthoredRepertoire::parse(pgn)?;
    let mut output = [String::new(), String::new(), String::new()];
    let mut lexer = PgnTokens::new(pgn);
    let mut start = 0;
    let mut depth = 0;
    let mut moves = false;
    loop {
        let before = pgn.len() - lexer.remaining().len();
        match lexer.next()? {
            Some(Token::VariationStart) => depth += 1,
            Some(Token::VariationEnd) => depth -= 1,
            Some(Token::San(_)) => moves = true,
            Some(Token::Tag(..)) if depth == 0 && moves => {
                partition_record(&pgn[start..before], &mut output)?;
                start = before;
                moves = false;
            }
            Some(Token::Result(_)) if depth == 0 => {
                let end = pgn.len() - lexer.remaining().len();
                partition_record(&pgn[start..end], &mut output)?;
                start = end;
                moves = false;
            }
            None => {
                if moves {
                    partition_record(&pgn[start..], &mut output)?;
                } else if !pgn[start..].trim().is_empty() {
                    return Err(invalid("Unexpected PGN text after the final chapter"));
                }
                break;
            }
            _ => {}
        }
    }
    let mut recombined = BTreeSet::new();
    for (group, document) in output.iter().enumerate() {
        if document.is_empty() {
            continue;
        }
        for route in AuthoredRepertoire::parse(document)?.routes {
            if opening_group(&route[0].1) != group {
                return Err(invalid("Partition has a route in the wrong opening group"));
            }
            recombined.insert(route);
        }
    }
    if recombined != original.routes.into_iter().collect() {
        return Err(invalid("Partition changed the set of authored routes"));
    }
    Ok(output)
}

fn opening_group(uci: &str) -> usize {
    match uci {
        "e2e4" => 0,
        "d2d4" => 1,
        _ => 2,
    }
}

fn partition_record(record: &str, output: &mut [String; 3]) -> Result<(), ChessError> {
    let games = parse_pgn(record)?;
    let game = &games[0];
    let group = opening_group(&game.plies[game.mainline[0]].uci);
    if game
        .routes()
        .all(|route| opening_group(&game.plies[route[0]].uci) == group)
    {
        output[group].push_str(record.trim());
        output[group].push_str("\n\n");
        return Ok(());
    }

    // PGN puts alternatives to the first move after that move and before the
    // second mainline move. Lift those RAVs as chapters, preserving their text.
    let mut lexer = PgnTokens::new(record);
    let mut main = String::new();
    let mut copied_to = 0;
    let mut header_end = 0;
    let mut root_variations = Vec::new();
    let mut root_start = None;
    let mut depth = 0;
    let mut main_plies = 0;
    loop {
        let Some(token) = lexer.next()? else { break };
        let after = record.len() - lexer.remaining().len();
        match token {
            Token::Tag(..) => header_end = after,
            Token::San(_) if depth == 0 => main_plies += 1,
            Token::VariationStart => {
                if depth == 0 && main_plies == 1 {
                    main.push_str(&record[copied_to..after - 1]);
                    main.push(' ');
                    root_start = Some(after);
                }
                depth += 1;
            }
            Token::VariationEnd => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = root_start.take()
                {
                    root_variations.push(record[start..after - 1].trim());
                    copied_to = after;
                }
            }
            _ => {}
        }
    }
    if root_variations.is_empty() {
        return Err(invalid("Could not separate the opening alternatives"));
    }
    main.push_str(&record[copied_to..]);
    partition_record(&main, output)?;
    for variation in root_variations {
        let result = if game.result.is_empty() {
            "*"
        } else {
            game.result
        };
        partition_record(
            &format!("{}\n\n{variation}\n{result}", &record[..header_end]),
            output,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_variations_split_without_losing_annotations_or_routes() {
        let [caro, slav, other] = partition_black_openings(
            "[Event \"My book\"]\n\n1.e4 {main} (1.d4 d5 $1 (1...c6 2.c4 d5)) (1.Nf3 d5? {other}) c6 *",
        ).unwrap();
        assert!(caro.contains("e4 {main}"));
        assert!(slav.contains("d5 $1 (1...c6"));
        assert!(other.contains("d5? {other}"));
    }
}
