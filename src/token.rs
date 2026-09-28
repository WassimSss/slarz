//! The words of a Slarz script, as produced by the lexer.

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

impl TokenKind {
    /// The keyword spelled by `word`, if it is one.
    pub fn keyword(word: &str) -> Option<TokenKind> {
        let kind = match word {
            "permissions" => Self::Permissions,
            "function" => Self::Function,
            "return" => Self::Return,
            "var" => Self::Var,
            "record" => Self::Record,
            "check" => Self::Check,
            "if" => Self::If,
            "else" => Self::Else,
            "while" => Self::While,
            "for" => Self::For,
            "in" => Self::In,
            "and" => Self::And,
            "or" => Self::Or,
            "not" => Self::Not,
            "true" => Self::True,
            "false" => Self::False,
            _ => return None,
        };
        Some(kind)
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
