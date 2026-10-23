use thiserror::Error;

use crate::error::CommandParseError;

/// One argv token and its byte span in the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unterminated quote at byte {position}")]
pub struct LexError {
    pub position: usize,
}

impl From<LexError> for CommandParseError {
    fn from(error: LexError) -> Self {
        Self::UnterminatedQuote {
            position: error.position,
        }
    }
}

/// Splits a command tail into tokens using the same quote rules as the JVM implementation.
///
/// Quotes only open a quoted segment at the start of a token. Quotes inside an existing token are
/// literal characters. There is no backslash escaping, glob expansion, or variable substitution.
pub fn split(input: &str) -> Result<Vec<Token>, LexError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut token_start = None;
    let mut token_end = 0;
    let mut quote = None;
    let mut quote_start = 0;

    for (offset, character) in input.char_indices() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else {
                if token_start.is_none() {
                    token_start = Some(offset);
                }
                current.push(character);
                token_end = offset + character.len_utf8();
            }
            continue;
        }

        if character.is_whitespace() {
            if let Some(start) = token_start.take() {
                if !current.is_empty() {
                    tokens.push(Token {
                        value: std::mem::take(&mut current),
                        start,
                        end: token_end,
                    });
                } else {
                    current.clear();
                }
            }
            continue;
        }

        if character == '"' || character == '\'' {
            if current.is_empty() {
                quote = Some(character);
                quote_start = offset;
                if token_start.is_none() {
                    token_start = Some(offset);
                }
                token_end = offset;
            } else {
                current.push(character);
                token_end = offset + character.len_utf8();
            }
            continue;
        }

        if token_start.is_none() {
            token_start = Some(offset);
        }
        current.push(character);
        token_end = offset + character.len_utf8();
    }

    if quote.is_some() {
        return Err(LexError {
            position: quote_start,
        });
    }

    if let Some(start) = token_start
        && !current.is_empty()
    {
        tokens.push(Token {
            value: current,
            start,
            end: token_end,
        });
    }

    Ok(tokens)
}
