//! Turns source text into tokens.

use std::iter::Peekable;
use std::str::CharIndices;

use crate::token::{ForeignSymbol, Span, Token, TokenKind};

/// Why the source could not be split into tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexErrorKind {
    UnexpectedCharacter(char),
    UnterminatedText,
    UnknownEscape(char),
    LeadingZero,
    NumberTooLarge,
}

/// A lexing error and the part of the source it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

/// Splits `source` into tokens. The last token is always `EndOfFile`.
pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        source,
        chars: source.char_indices().peekable(),
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    let end = source.len();
    tokens.push(Token {
        kind: TokenKind::EndOfFile,
        span: Span { start: end, end },
    });
    Ok(tokens)
}

struct Lexer<'a> {
    source: &'a str,
    // Characters with their byte offset, so every token knows its span.
    chars: Peekable<CharIndices<'a>>,
}

impl<'a> Lexer<'a> {
    fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_whitespace_and_comments();
        let Some((start, c)) = self.chars.next() else {
            return Ok(None);
        };
        let kind = match c {
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            ':' => TokenKind::Colon,
            '.' => TokenKind::Dot,
            // Longer symbols first: a guard only consumes the next character when it matches.
            '+' if self.consume_if('+') => TokenKind::Foreign(ForeignSymbol::PlusPlus),
            '+' if self.consume_if('=') => TokenKind::Foreign(ForeignSymbol::PlusEqual),
            '+' => TokenKind::Plus,
            '-' if self.consume_if('>') => TokenKind::Arrow,
            '-' if self.consume_if('-') => TokenKind::Foreign(ForeignSymbol::MinusMinus),
            '-' if self.consume_if('=') => TokenKind::Foreign(ForeignSymbol::MinusEqual),
            '-' => TokenKind::Minus,
            '*' if self.consume_if('=') => TokenKind::Foreign(ForeignSymbol::StarEqual),
            '*' => TokenKind::Star,
            '/' if self.consume_if('=') => TokenKind::Foreign(ForeignSymbol::SlashEqual),
            '/' => TokenKind::Slash,
            '=' if self.consume_if('=') => {
                if self.consume_if('=') {
                    TokenKind::Foreign(ForeignSymbol::TripleEqual)
                } else {
                    TokenKind::EqualEqual
                }
            }
            '=' => TokenKind::Equal,
            '!' if self.consume_if('=') => {
                if self.consume_if('=') {
                    TokenKind::Foreign(ForeignSymbol::BangDoubleEqual)
                } else {
                    TokenKind::BangEqual
                }
            }
            '!' => TokenKind::Foreign(ForeignSymbol::Bang),
            '<' if self.consume_if('=') => TokenKind::LessEqual,
            '<' => TokenKind::Less,
            '>' if self.consume_if('=') => TokenKind::GreaterEqual,
            '>' => TokenKind::Greater,
            '&' if self.consume_if('&') => TokenKind::Foreign(ForeignSymbol::AndAnd),
            '|' if self.consume_if('|') => TokenKind::Foreign(ForeignSymbol::OrOr),
            '"' => self.text(start)?,
            c if c.is_ascii_digit() => self.number(start)?,
            // Names are ASCII only: no look-alike letters from other alphabets.
            c if c.is_ascii_alphabetic() || c == '_' => self.word(start),
            other => return Err(self.error(LexErrorKind::UnexpectedCharacter(other), start)),
        };
        Ok(Some(Token {
            kind,
            span: self.span_from(start),
        }))
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.chars.next();
                }
                Some('/') if self.peek_second() == Some('/') => self.consume_while(|c| c != '\n'),
                _ => return,
            }
        }
    }

    fn word(&mut self, start: usize) -> TokenKind {
        self.consume_while(|c| c.is_ascii_alphanumeric() || c == '_');
        let word = self.slice_from(start);
        TokenKind::keyword(word).unwrap_or_else(|| TokenKind::Identifier(word.to_string()))
    }

    fn number(&mut self, start: usize) -> Result<TokenKind, LexError> {
        self.consume_while(|c| c.is_ascii_digit());
        let integer_part = self.slice_from(start);
        if integer_part.len() > 1 && integer_part.starts_with('0') {
            return Err(self.error(LexErrorKind::LeadingZero, start));
        }

        // `5.` and `.5` are not numbers: a dot needs digits on both sides.
        let has_fraction =
            self.peek() == Some('.') && self.peek_second().is_some_and(|c| c.is_ascii_digit());
        let kind = if has_fraction {
            self.chars.next();
            self.consume_while(|c| c.is_ascii_digit());
            self.slice_from(start).parse().map(TokenKind::Float).ok()
        } else {
            integer_part.parse().map(TokenKind::Integer).ok()
        };
        kind.ok_or_else(|| self.error(LexErrorKind::NumberTooLarge, start))
    }

    fn text(&mut self, start: usize) -> Result<TokenKind, LexError> {
        let mut text = String::new();
        loop {
            match self.chars.next() {
                // Text stays on one line, so a missing quote is caught right away.
                None | Some((_, '\n')) => {
                    return Err(self.error(LexErrorKind::UnterminatedText, start));
                }
                Some((_, '"')) => return Ok(TokenKind::Text(text)),
                Some((escape_start, '\\')) => text.push(self.escape(escape_start)?),
                Some((_, c)) => text.push(c),
            }
        }
    }

    fn escape(&mut self, start: usize) -> Result<char, LexError> {
        match self.chars.next() {
            Some((_, 'n')) => Ok('\n'),
            Some((_, 't')) => Ok('\t'),
            Some((_, '"')) => Ok('"'),
            Some((_, '\\')) => Ok('\\'),
            Some((_, other)) => Err(self.error(LexErrorKind::UnknownEscape(other), start)),
            None => Err(self.error(LexErrorKind::UnterminatedText, start)),
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().map(|&(_, c)| c)
    }

    fn peek_second(&self) -> Option<char> {
        let mut ahead = self.chars.clone();
        ahead.next();
        ahead.next().map(|(_, c)| c)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        let matches = self.peek() == Some(expected);
        if matches {
            self.chars.next();
        }
        matches
    }

    fn consume_while(&mut self, keep: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&keep) {
            self.chars.next();
        }
    }

    fn position(&mut self) -> usize {
        self.chars.peek().map_or(self.source.len(), |&(i, _)| i)
    }

    fn slice_from(&mut self, start: usize) -> &'a str {
        let end = self.position();
        self.source.get(start..end).unwrap_or_default()
    }

    fn span_from(&mut self, start: usize) -> Span {
        Span {
            start,
            end: self.position(),
        }
    }

    fn error(&mut self, kind: LexErrorKind, start: usize) -> LexError {
        LexError {
            kind,
            span: self.span_from(start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::ForeignSymbol::*;
    use crate::token::TokenKind::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn error(source: &str) -> LexErrorKind {
        tokenize(source).unwrap_err().kind
    }

    fn identifier(name: &str) -> TokenKind {
        Identifier(name.to_string())
    }

    #[test]
    fn declaration() {
        assert_eq!(
            kinds("prenom: Text = \"wassim\";"),
            vec![
                identifier("prenom"),
                Colon,
                identifier("Text"),
                Equal,
                Text("wassim".to_string()),
                Semicolon,
                EndOfFile,
            ]
        );
    }

    #[test]
    fn keywords_and_contextual_words() {
        assert_eq!(
            kinds("var file function"),
            vec![Var, identifier("file"), Function, EndOfFile]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(
            kinds("== != <= >= -> = < > - + * /"),
            vec![
                EqualEqual,
                BangEqual,
                LessEqual,
                GreaterEqual,
                Arrow,
                Equal,
                Less,
                Greater,
                Minus,
                Plus,
                Star,
                Slash,
                EndOfFile,
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(
            kinds("42 2.5 0 5."),
            vec![
                Integer(42),
                Float(2.5),
                Integer(0),
                Integer(5),
                Dot,
                EndOfFile
            ]
        );
    }

    #[test]
    fn text_escapes() {
        assert_eq!(
            kinds(r#""a\n\t\"b\\""#),
            vec![Text("a\n\t\"b\\".to_string()), EndOfFile]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            kinds("a // note / x\nb"),
            vec![identifier("a"), identifier("b"), EndOfFile]
        );
    }

    #[test]
    fn windows_and_unix_line_endings_are_equivalent() {
        assert_eq!(kinds("a;\r\nb;"), kinds("a;\nb;"));
    }

    #[test]
    fn foreign_symbols() {
        assert_eq!(
            kinds("&& || ! === !== ++ -- += -= *= /="),
            vec![
                Foreign(AndAnd),
                Foreign(OrOr),
                Foreign(Bang),
                Foreign(TripleEqual),
                Foreign(BangDoubleEqual),
                Foreign(PlusPlus),
                Foreign(MinusMinus),
                Foreign(PlusEqual),
                Foreign(MinusEqual),
                Foreign(StarEqual),
                Foreign(SlashEqual),
                EndOfFile,
            ]
        );
    }

    #[test]
    fn spans_point_at_the_token() {
        let source = "total = 42;\n  next";
        let tokens = tokenize(source).unwrap();
        assert_eq!(tokens[2].span, Span { start: 8, end: 10 });
        assert_eq!(tokens[4].span.line_column(source), (2, 3));
    }

    #[test]
    fn errors() {
        assert_eq!(error("\"abc"), LexErrorKind::UnterminatedText);
        assert_eq!(error("\"abc\ndef\""), LexErrorKind::UnterminatedText);
        assert_eq!(error(r#""\q""#), LexErrorKind::UnknownEscape('q'));
        assert_eq!(error("café"), LexErrorKind::UnexpectedCharacter('é'));
        assert_eq!(error("a & b"), LexErrorKind::UnexpectedCharacter('&'));
        assert_eq!(error("007"), LexErrorKind::LeadingZero);
        assert_eq!(error("99999999999999999999"), LexErrorKind::NumberTooLarge);
    }
}
