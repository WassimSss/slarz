//! The words of a Slarz script, as produced by the lexer.

use std::fmt;

/// A region of the source text, in byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// Line and column where the span starts, both counted from 1.
    pub fn line_column(&self, source: &str) -> (usize, usize) {
        let before = source.get(..self.start).unwrap_or(source);
        let line = before.matches('\n').count() + 1;
        let current_line = before.rsplit('\n').next().unwrap_or("");
        (line, current_line.chars().count() + 1)
    }

    /// The span that goes from the start of `self` to the end of `other`.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
        }
    }
}

/// A word of the script and where it appears.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Every kind of word the lexer can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals and names.
    Integer(i64),
    Float(f64),
    Text(String),
    Identifier(String),

    // Keywords. Permission words (`read`, `file`...) are not listed: they only
    // mean something inside a `permissions` block, which the parser handles.
    Permissions,
    Function,
    Return,
    Var,
    Record,
    Check,
    Otherwise,
    If,
    Else,
    While,
    For,
    In,
    And,
    Or,
    Not,
    True,
    False,

    // Punctuation.
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    Arrow,

    // Operators.
    Plus,
    Minus,
    Star,
    Slash,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    /// A symbol from another language, recognized on purpose so that the
    /// error can suggest the Slarz equivalent.
    Foreign(ForeignSymbol),

    EndOfFile,
}

const KEYWORDS: [(&str, TokenKind); 17] = [
    ("permissions", TokenKind::Permissions),
    ("function", TokenKind::Function),
    ("return", TokenKind::Return),
    ("var", TokenKind::Var),
    ("record", TokenKind::Record),
    ("check", TokenKind::Check),
    ("otherwise", TokenKind::Otherwise),
    ("if", TokenKind::If),
    ("else", TokenKind::Else),
    ("while", TokenKind::While),
    ("for", TokenKind::For),
    ("in", TokenKind::In),
    ("and", TokenKind::And),
    ("or", TokenKind::Or),
    ("not", TokenKind::Not),
    ("true", TokenKind::True),
    ("false", TokenKind::False),
];

impl TokenKind {
    /// The keyword spelled by `word`, if it is one.
    pub fn keyword(word: &str) -> Option<TokenKind> {
        KEYWORDS
            .iter()
            .find(|(spelling, _)| *spelling == word)
            .map(|(_, kind)| kind.clone())
    }
}

/// How a token is named in error messages: "found `;`", "found the name `x`".
impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spelling = match self {
            Self::Integer(value) => return write!(f, "the number `{value}`"),
            Self::Float(value) => return write!(f, "the number `{value}`"),
            Self::Text(text) => return write!(f, "the text {text:?}"),
            Self::Identifier(name) => return write!(f, "the name `{name}`"),
            Self::EndOfFile => return write!(f, "the end of the script"),
            Self::Foreign(symbol) => symbol.spelling(),
            Self::LeftParen => "(",
            Self::RightParen => ")",
            Self::LeftBrace => "{",
            Self::RightBrace => "}",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Comma => ",",
            Self::Semicolon => ";",
            Self::Colon => ":",
            Self::Dot => ".",
            Self::Arrow => "->",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Equal => "=",
            Self::EqualEqual => "==",
            Self::BangEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            keyword => KEYWORDS
                .iter()
                .find(|(_, kind)| kind == keyword)
                .map_or("?", |(spelling, _)| spelling),
        };
        write!(f, "`{spelling}`")
    }
}

/// Symbols Slarz does not use but other languages do (`&&`, `++`, `===`...),
/// each with a known Slarz equivalent to suggest in the error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignSymbol {
    AndAnd,
    OrOr,
    Bang,
    TripleEqual,
    BangDoubleEqual,
    PlusPlus,
    MinusMinus,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
}

impl ForeignSymbol {
    pub fn spelling(self) -> &'static str {
        match self {
            Self::AndAnd => "&&",
            Self::OrOr => "||",
            Self::Bang => "!",
            Self::TripleEqual => "===",
            Self::BangDoubleEqual => "!==",
            Self::PlusPlus => "++",
            Self::MinusMinus => "--",
            Self::PlusEqual => "+=",
            Self::MinusEqual => "-=",
            Self::StarEqual => "*=",
            Self::SlashEqual => "/=",
        }
    }

    /// What to write in Slarz instead.
    pub fn suggestion(self) -> &'static str {
        match self {
            Self::AndAnd => "use `and`",
            Self::OrOr => "use `or`",
            Self::Bang => "use `not`",
            Self::TripleEqual => "Slarz has a single equality, use `==`",
            Self::BangDoubleEqual => "use `!=`",
            Self::PlusPlus => "write `x = x + 1;`",
            Self::MinusMinus => "write `x = x - 1;`",
            Self::PlusEqual => "write `x = x + value;`",
            Self::MinusEqual => "write `x = x - value;`",
            Self::StarEqual => "write `x = x * value;`",
            Self::SlashEqual => "write `x = x / value;`",
        }
    }
}
