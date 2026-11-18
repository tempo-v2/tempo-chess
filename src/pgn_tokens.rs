//! Read PGN tags, move text, comments and variation delimiters.

use crate::ChessError;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Token<'a> {
    Tag(&'a str, &'a str),
    MoveNumber { number: u16, black: bool },
    San(&'a str),
    Result(&'a str),
    Comment(&'a str),
    VariationStart,
    VariationEnd,
}

pub(super) struct PgnTokens<'a> {
    rest: &'a str,
}

impl<'a> PgnTokens<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self {
            rest: input.strip_prefix('\u{feff}').unwrap_or(input),
        }
    }

    pub(super) fn next(&mut self) -> Result<Option<Token<'a>>, ChessError> {
        loop {
            self.rest = self.rest.trim_start();
            match self.rest.as_bytes().first() {
                None => return Ok(None),
                Some(b'{') => {
                    let Some((comment, rest)) = self.rest[1..].split_once('}') else {
                        self.rest = "";
                        return Err(invalid("unclosed PGN comment"));
                    };
                    self.rest = rest;
                    return Ok(Some(Token::Comment(comment.trim())));
                }
                Some(b';') => {
                    let comment_line = &self.rest[1..];
                    let (comment, rest) = comment_line.split_at(
                        comment_line
                            .find(['\n', '\r'])
                            .unwrap_or(comment_line.len()),
                    );
                    self.rest = rest;
                    return Ok(Some(Token::Comment(comment.trim())));
                }
                Some(b'(') => {
                    self.rest = &self.rest[1..];
                    return Ok(Some(Token::VariationStart));
                }
                Some(b')') => {
                    self.rest = &self.rest[1..];
                    return Ok(Some(Token::VariationEnd));
                }
                Some(b'}' | b']') => {
                    self.rest = &self.rest[1..];
                    return Err(invalid("unexpected PGN closing delimiter"));
                }
                Some(b'[') => {
                    return self.tag().map(Some).inspect_err(|_| {
                        // Tag strings cannot cross a line. Recover at the next line.
                        self.rest = self
                            .rest
                            .find(['\n', '\r'])
                            .map_or("", |end| &self.rest[end..]);
                    });
                }
                Some(b'$') => {
                    self.rest = &self.rest[1..];
                    let count = self.rest.bytes().take_while(u8::is_ascii_digit).count();
                    if count == 0 {
                        return Err(invalid("PGN numeric annotation requires digits"));
                    }
                    self.rest = &self.rest[count..];
                }
                Some(_) => {
                    if let Some(token) = self.move_number().inspect_err(|_| {
                        self.take_word();
                    })? {
                        return Ok(Some(token));
                    }
                    let word = self.take_word();
                    if is_result(word) {
                        return Ok(Some(Token::Result(word)));
                    }
                    let san = word.trim_end_matches(['!', '?']);
                    if !matches!(
                        &word[san.len()..],
                        "" | "!" | "?" | "!!" | "??" | "!?" | "?!"
                    ) {
                        return Err(invalid("invalid PGN annotation glyph"));
                    }
                    if !san.is_empty() {
                        return Ok(Some(Token::San(san)));
                    }
                }
            }
        }
    }

    fn take_word(&mut self) -> &'a str {
        let end = self
            .rest
            .find(|c: char| c.is_whitespace() || "{};()[]$".contains(c))
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        word
    }

    fn move_number(&mut self) -> Result<Option<Token<'a>>, ChessError> {
        let digits = self.rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !self.rest[digits..].starts_with('.') {
            return Ok(None);
        }
        let dots = self.rest[digits..]
            .bytes()
            .take_while(|b| *b == b'.')
            .count();
        let number = self.rest[..digits]
            .parse()
            .map_err(|_| invalid("invalid PGN move number"))?;
        if !matches!(dots, 1 | 3) || number == 0 {
            return Err(invalid("invalid PGN move number"));
        }
        self.rest = &self.rest[digits + dots..];
        Ok(Some(Token::MoveNumber {
            number,
            black: dots == 3,
        }))
    }

    fn tag(&mut self) -> Result<Token<'a>, ChessError> {
        let inner = self.rest[1..].trim_start();
        let name_end = inner
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        let (name, after_name) = inner.split_at(name_end);
        let quoted = after_name.trim_start();
        let value = quoted
            .strip_prefix('"')
            .filter(|_| !name.is_empty() && quoted.len() < after_name.len())
            .ok_or_else(|| invalid("invalid PGN tag"))?;
        let mut escaped = false;
        for (index, c) in value.char_indices() {
            match c {
                '\r' | '\n' => break,
                '"' | '\\' if escaped => escaped = false,
                _ if escaped => return Err(invalid("invalid escape in PGN tag")),
                '\\' => escaped = true,
                '"' => {
                    self.rest = value[index + 1..]
                        .trim_start()
                        .strip_prefix(']')
                        .ok_or_else(|| invalid("missing PGN tag closer"))?;
                    return Ok(Token::Tag(name, &value[..index]));
                }
                _ => {}
            }
        }
        Err(invalid("unclosed PGN tag value"))
    }
}

pub(super) fn is_result(word: &str) -> bool {
    matches!(word, "1-0" | "0-1" | "1/2-1/2" | "*")
}

fn invalid(message: &str) -> ChessError {
    ChessError::InvalidContent(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_san_stays_visible_around_comments_and_nested_variations() {
        let mut pgn = PgnTokens::new(
            "[Event \"Québec \\\"open\\\"\"]\n1.e4!? $4 (1. c4 {example} (1. Nf3)) ; comment\r\n1...c5 *",
        );
        for token in [
            Token::Tag("Event", r#"Québec \"open\""#),
            Token::MoveNumber {
                number: 1,
                black: false,
            },
            Token::San("e4"),
            Token::VariationStart,
            Token::MoveNumber {
                number: 1,
                black: false,
            },
            Token::San("c4"),
            Token::Comment("example"),
            Token::VariationStart,
            Token::MoveNumber {
                number: 1,
                black: false,
            },
            Token::San("Nf3"),
            Token::VariationEnd,
            Token::VariationEnd,
            Token::Comment("comment"),
            Token::MoveNumber {
                number: 1,
                black: true,
            },
            Token::San("c5"),
            Token::Result("*"),
        ] {
            assert_eq!(pgn.next().unwrap(), Some(token));
        }
        assert_eq!(pgn.next().unwrap(), None);
    }
}
