//! Reads JSON text (RFC 8259) into a tree, with one function per kind of
//! value. Knows nothing about Slarz.

use std::collections::HashSet;
use std::fmt;

/// Each level of nesting is a recursive call: without a limit, a document
/// like `[[[[...` a million levels deep would overflow the stack.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Kept as written: an identifier like `1234567890123456789` would lose
    /// digits as an `f64`.
    Number(String),
    Text(String),
    List(Vec<Json>),
    /// Keys in their original order, each one at most once.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value of `key`, if this is an object that has it.
    pub fn field(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// What kind of value this is, as a sentence fragment: "a number".
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "a boolean",
            Self::Number(_) => "a number",
            Self::Text(_) => "a text",
            Self::List(_) => "a list",
            Self::Object(_) => "an object",
        }
    }
}

/// Writes compact JSON, keys in their original order.
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(number) => write!(f, "{number}"),
            Self::Text(text) => write_text(f, text),
            Self::List(items) => {
                write!(f, "[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Self::Object(fields) => {
                write!(f, "{{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ",")?;
                    }
                    write_text(f, key)?;
                    write!(f, ":{value}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_text(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    write!(f, "\"")?;
    for character in text.chars() {
        match character {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\r' => write!(f, "\\r")?,
            '\t' => write!(f, "\\t")?,
            control if control < ' ' => write!(f, "\\u{:04x}", u32::from(control))?,
            other => write!(f, "{other}")?,
        }
    }
    write!(f, "\"")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid JSON at line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

/// Reads a whole JSON document: one value, surrounded only by whitespace.
pub fn parse(text: &str) -> Result<Json, JsonError> {
    let mut parser = Parser {
        text,
        position: 0,
        depth: 0,
    };
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.position < text.len() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    /// In bytes. Every character JSON gives a meaning to is ASCII, so the
    /// parser only stops on character boundaries.
    position: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn value(&mut self) -> Result<Json, JsonError> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.nested(Self::object),
            Some('[') => self.nested(Self::list),
            Some('"') => Ok(Json::Text(self.text_value()?)),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some('-' | '0'..='9') => self.number(),
            Some(_) => Err(self.error("expected a value")),
            None => Err(self.error("the text ends where a value was expected")),
        }
    }

    fn nested(
        &mut self,
        parse: fn(&mut Self) -> Result<Json, JsonError>,
    ) -> Result<Json, JsonError> {
        if self.depth == MAX_DEPTH {
            return Err(self.error(&format!("nested more than {MAX_DEPTH} levels deep")));
        }
        self.depth += 1;
        let value = parse(self);
        self.depth -= 1;
        value
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.consume('{');
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        self.skip_whitespace();
        if self.consume('}') {
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_whitespace();
            let key_position = self.position;
            if self.peek() != Some('"') {
                return Err(self.error("expected a key in double quotes"));
            }
            let key = self.text_value()?;
            // Some readers keep the first duplicate, others the last: an
            // attacker can make a checker and an actor see different values.
            if !seen.insert(key.clone()) {
                return Err(
                    self.error_at(key_position, &format!("the key \"{key}\" appears twice"))
                );
            }
            self.skip_whitespace();
            if !self.consume(':') {
                return Err(self.error("expected `:` after the key"));
            }
            fields.push((key, self.value()?));
            self.skip_whitespace();
            if self.consume('}') {
                return Ok(Json::Object(fields));
            }
            if !self.consume(',') {
                return Err(self.error("expected `,` or `}`"));
            }
        }
    }

    fn list(&mut self) -> Result<Json, JsonError> {
        self.consume('[');
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.consume(']') {
            return Ok(Json::List(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_whitespace();
            if self.consume(']') {
                return Ok(Json::List(items));
            }
            if !self.consume(',') {
                return Err(self.error("expected `,` or `]`"));
            }
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if !self.rest().starts_with(word) {
            return Err(self.error("expected a value"));
        }
        self.position += word.len();
        Ok(value)
    }

    // `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`: no `+1`, `.5`,
    // `5.` or `01`.
    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.position;
        self.consume('-');
        if !self.consume('0') && !self.digits() {
            return Err(self.error("expected a digit"));
        }
        if self.consume('.') && !self.digits() {
            return Err(self.error("expected a digit after `.`"));
        }
        if self.consume('e') || self.consume('E') {
            let _ = self.consume('+') || self.consume('-');
            if !self.digits() {
                return Err(self.error("expected a digit in the exponent"));
            }
        }
        Ok(Json::Number(self.text[start..self.position].to_string()))
    }

    fn digits(&mut self) -> bool {
        let count = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        self.position += count;
        count > 0
    }

    fn text_value(&mut self) -> Result<String, JsonError> {
        self.consume('"');
        let mut text = String::new();
        loop {
            let Some(character) = self.peek() else {
                return Err(self.error("the text is missing its closing `\"`"));
            };
            self.position += character.len_utf8();
            match character {
                '"' => return Ok(text),
                '\\' => text.push(self.escape()?),
                control if control < ' ' => {
                    return Err(self.error_at(
                        self.position - 1,
                        "control characters must be escaped in text, like `\\n`",
                    ));
                }
                other => text.push(other),
            }
        }
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        let start = self.position;
        let Some(character) = self.peek() else {
            return Err(self.error("the text ends inside an escape"));
        };
        self.position += character.len_utf8();
        match character {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{8}'),
            'f' => Ok('\u{c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.unicode_escape(),
            _ => Err(self.error_at(start, "unknown escape")),
        }
    }

    // `\u` gives one UTF-16 unit: a character beyond U+FFFF comes as a pair
    // of them, like `😀` for 😀.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let start = self.position;
        let first = self.hex_unit()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if !self.rest().starts_with("\\u") {
                return Err(self.error_at(start, "this `\\u` escape needs its second half"));
            }
            self.position += 2;
            let second = self.hex_unit()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(self.error_at(start, "invalid pair of `\\u` escapes"));
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        // A second half alone (U+DC00 to U+DFFF) is not a character.
        char::from_u32(code)
            .ok_or_else(|| self.error_at(start, "this `\\u` escape is not a character"))
    }

    fn hex_unit(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .rest()
            .get(..4)
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("expected 4 hexadecimal digits after `\\u`"))?;
        let unit = u32::from_str_radix(digits, 16)
            .map_err(|_| self.error("expected 4 hexadecimal digits after `\\u`"))?;
        self.position += 4;
        Ok(unit)
    }

    fn skip_whitespace(&mut self) {
        let count = self
            .rest()
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
            .count();
        self.position += count;
    }

    fn rest(&self) -> &'a str {
        &self.text[self.position..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn consume(&mut self, expected: char) -> bool {
        let matches = self.peek() == Some(expected);
        if matches {
            self.position += expected.len_utf8();
        }
        matches
    }

    fn error(&self, message: &str) -> JsonError {
        self.error_at(self.position, message)
    }

    fn error_at(&self, position: usize, message: &str) -> JsonError {
        let before = self.text.get(..position).unwrap_or(self.text);
        JsonError {
            message: message.to_string(),
            line: before.matches('\n').count() + 1,
            column: before.chars().rev().take_while(|&c| c != '\n').count() + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Json {
        Json::Text(value.to_string())
    }

    fn number(value: &str) -> Json {
        Json::Number(value.to_string())
    }

    fn message(source: &str) -> String {
        parse(source).unwrap_err().message
    }

    #[test]
    fn literals() {
        assert_eq!(parse("true"), Ok(Json::Bool(true)));
        assert_eq!(parse("false"), Ok(Json::Bool(false)));
        assert_eq!(parse(" \n null \t"), Ok(Json::Null));
        assert_eq!(message("tru"), "expected a value");
        assert_eq!(message("true x"), "unexpected text after the value");
        assert_eq!(message(""), "the text ends where a value was expected");
    }

    #[test]
    fn numbers_are_kept_as_written() {
        assert_eq!(
            parse("1234567890123456789"),
            Ok(number("1234567890123456789"))
        );
        for valid in ["0", "-0", "42", "-3.5", "1e10", "2.5E-3", "1e+2"] {
            assert_eq!(parse(valid), Ok(number(valid)), "{valid}");
        }
        for invalid in ["+1", ".5", "5.", "01", "1e", "-", "NaN"] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn texts_and_escapes() {
        assert_eq!(parse(r#""café""#), Ok(text("café")));
        assert_eq!(parse(r#""a\"b\\c\/d\n\t""#), Ok(text("a\"b\\c/d\n\t")));
        assert_eq!(parse(r#""é""#), Ok(text("é")));
        assert_eq!(parse(r#""😀""#), Ok(text("😀")));
        assert_eq!(message(r#""open"#), "the text is missing its closing `\"`");
        assert_eq!(message(r#""\x""#), "unknown escape");
        assert_eq!(
            message(r#""\ud83d""#),
            "this `\\u` escape needs its second half"
        );
        assert_eq!(
            message(r#""\ude00""#),
            "this `\\u` escape is not a character"
        );
        assert_eq!(
            message(r#""\u12""#),
            "expected 4 hexadecimal digits after `\\u`"
        );
        assert_eq!(
            message("\"line\nbreak\""),
            "control characters must be escaped in text, like `\\n`"
        );
    }

    #[test]
    fn lists() {
        assert_eq!(parse("[]"), Ok(Json::List(vec![])));
        assert_eq!(
            parse("[1, \"a\", [true]]"),
            Ok(Json::List(vec![
                number("1"),
                text("a"),
                Json::List(vec![Json::Bool(true)]),
            ]))
        );
        assert_eq!(message("[1,]"), "expected a value");
        assert_eq!(message("[1 2]"), "expected `,` or `]`");
    }

    #[test]
    fn objects_keep_their_order() {
        assert_eq!(
            parse(r#"{"login": "WassimSss", "company": null, "repos": [{"stars": 3}]}"#),
            Ok(Json::Object(vec![
                ("login".to_string(), text("WassimSss")),
                ("company".to_string(), Json::Null),
                (
                    "repos".to_string(),
                    Json::List(vec![Json::Object(vec![("stars".to_string(), number("3"))])]),
                ),
            ]))
        );
        assert_eq!(parse("{ }"), Ok(Json::Object(vec![])));
        assert_eq!(message("{login: 1}"), "expected a key in double quotes");
        assert_eq!(message(r#"{"a" 1}"#), "expected `:` after the key");
        assert_eq!(message(r#"{"a": 1,}"#), "expected a key in double quotes");
    }

    #[test]
    fn duplicate_keys_are_refused() {
        let error = parse(r#"{"admin": false, "admin": true}"#).unwrap_err();
        assert_eq!(error.message, "the key \"admin\" appears twice");
        assert_eq!(error.column, 18);
    }

    #[test]
    fn nesting_is_limited() {
        let deep = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&deep).is_ok());
        let too_deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(message(&too_deep), "nested more than 128 levels deep");
        let attack = "[".repeat(1_000_000);
        assert!(parse(&attack).is_err());
    }

    #[test]
    fn writes_compact_json_that_reads_back() {
        let source = r#"{ "name": "a \"b\"\n\u0001é", "n": -1.5e3, "list": [true, null, {}] }"#;
        let written = parse(source).unwrap().to_string();
        assert_eq!(
            written,
            r#"{"name":"a \"b\"\n\u0001é","n":-1.5e3,"list":[true,null,{}]}"#
        );
        assert_eq!(parse(&written), parse(source));
    }

    #[test]
    fn fields() {
        let user = parse(r#"{"login": "WassimSss", "company": null}"#).unwrap();
        assert_eq!(user.field("login"), Some(&text("WassimSss")));
        assert_eq!(user.field("company"), Some(&Json::Null));
        assert_eq!(user.field("email"), None);
        assert_eq!(Json::List(vec![]).field("login"), None);
    }

    #[test]
    fn errors_point_at_line_and_column() {
        let error = parse("{\n  \"a\": tru\n}").unwrap_err();
        assert_eq!((error.line, error.column), (2, 8));
        assert_eq!(
            error.to_string(),
            "invalid JSON at line 2, column 8: expected a value"
        );
    }
}
