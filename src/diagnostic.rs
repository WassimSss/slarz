//! Every message the language shows when a script cannot run, for every
//! stage, in one place, and the way an error is printed.
//!
//! The error types stay in their stage (`lexer`, `parser`, `interpreter`):
//! they say *what* went wrong. This module says it in words, so that a
//! message can be read, compared and improved without opening the code that
//! detects the error.

use std::fmt;

use crate::interpreter::{MAX_CALL_DEPTH, RuntimeErrorKind};
use crate::lexer::LexErrorKind;
use crate::parser::ParseErrorKind;
use crate::token::{ForeignSymbol, KEYWORDS, Span, TokenKind};

/// The line printed for an error: `error: script.slz:3:5: <message>`.
pub fn render(path: &str, source: &str, span: Span, message: impl fmt::Display) -> String {
    let (line, column) = span.line_column(source);
    format!("error: {path}:{line}:{column}: {message}")
}

// ----- Lexer -----

impl fmt::Display for LexErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedCharacter(c) => write!(f, "unexpected character {c:?}"),
            Self::UnterminatedText => {
                write!(
                    f,
                    "text is not closed: add a `\"` before the end of the line"
                )
            }
            Self::UnknownEscape(c) => {
                write!(
                    f,
                    "unknown escape `\\{c}`: use `\\n`, `\\t`, `\\\"` or `\\\\`"
                )
            }
            Self::LeadingZero => write!(f, "a number cannot start with 0: write `7`, not `07`"),
            Self::NumberTooLarge => {
                write!(f, "number is too large: integers go up to {}", i64::MAX)
            }
        }
    }
}

// ----- Parser -----

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expected { expected, found } => write!(f, "expected {expected}, found {found}"),
            Self::Foreign(symbol) => write!(
                f,
                "`{}` is not Slarz: {}",
                symbol.spelling(),
                symbol.suggestion()
            ),
            Self::ForeignKeyword(word) if word == "let" || word == "const" => write!(
                f,
                "Slarz has no `{word}`: write `name: Type = value;` for a constant, \
                 or `var name: Type = value;` for a variable"
            ),
            Self::ForeignKeyword(word) => {
                write!(f, "functions are declared with `function`, not `{word}`")
            }
            Self::ChainedComparison => {
                write!(f, "comparisons cannot be chained: write `a < b and b < c`")
            }
            Self::UnknownPermission(word) => write!(
                f,
                "unknown permission `{word}`: use `read`, `write`, `network` or `env`"
            ),
            Self::UnknownHttpMethod(method) => write!(
                f,
                "unknown HTTP method `{method}`: use GET, POST, PUT, PATCH or DELETE"
            ),
        }
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

/// The false friends: what to write in Slarz instead of a symbol from another
/// language.
impl ForeignSymbol {
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

// ----- Running -----

/// An absence has no reason of its own: the error points at the `check`.
pub(crate) const NO_VALUE: &str = "`check` found no value here: give one with `otherwise`, or handle \
                        the absence with `if name: Type = ... { } else { }`";

impl fmt::Display for RuntimeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedVariable(name) => write!(
                f,
                "`{name}` is not defined: declare it first with `{name}: Type = value;`"
            ),
            Self::UndefinedFunction(name) => write!(f, "there is no function called `{name}`"),
            Self::AlreadyDeclared(name) => write!(
                f,
                "`{name}` already exists: a name cannot be declared twice, pick another one"
            ),
            Self::AssignToConstant(name) => write!(
                f,
                "`{name}` is a constant: declare it with `var` if it needs to change"
            ),
            Self::TypeMismatch { expected, found } => {
                write!(f, "expected a value of type `{expected}`, found `{found}`")
            }
            Self::UnknownType(name) if name == "String" || name == "str" => {
                write!(f, "unknown type `{name}`: the text type is called `Text`")
            }
            Self::UnknownType(name) => write!(
                f,
                "unknown type `{name}`: the available types are `Int`, `Float`, `Text`, `Bool`, \
                 `Json`, `List<T>`, `Optional<T>` and `Result<T, Error>`"
            ),
            Self::InvalidOperands {
                operator,
                left,
                right,
            } => write!(
                f,
                "`{operator}` cannot be used between `{left}` and `{right}`"
            ),
            Self::InvalidOperand { operator, operand } => {
                write!(f, "`{operator}` cannot be used on `{operand}`")
            }
            Self::IntegerDivision => write!(
                f,
                "`/` only divides `Float` values, because languages disagree on what `7 / 2` \
                 means for integers: write `quotient(7, 2)` for a whole number, or `7.0 / 2.0`"
            ),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::Overflow => write!(f, "integer overflow: the result does not fit in an `Int`"),
            Self::InexactConversion { value, target } => write!(
                f,
                "`{value}` cannot be turned into a `{target}` without changing its value"
            ),
            Self::InvalidArgument { function, reason } => write!(f, "`{function}`: {reason}"),
            Self::WrongArgumentCount {
                function,
                expected,
                found,
            } => write!(
                f,
                "`{function}` expects {expected} argument(s), but got {found}"
            ),
            Self::MissingReturn(name) => write!(
                f,
                "function `{name}` ended without returning a value: add a `return` statement"
            ),
            Self::UnexpectedReturnValue(name) => write!(
                f,
                "function `{name}` has no return type, so it cannot return a value: \
                 add `-> Type` to its declaration"
            ),
            Self::ReturnOutsideFunction => write!(f, "`return` can only be used inside a function"),
            Self::NestedFunction => write!(
                f,
                "functions must be declared at the top level of the script, not inside a block"
            ),
            Self::TooDeepRecursion => write!(
                f,
                "too many nested function calls (the limit is {MAX_CALL_DEPTH})"
            ),
            Self::NotSupportedYet(what) => write!(f, "{what}"),
            Self::OutputFailed => write!(f, "could not write the output"),
            Self::PermissionDenied { access, path } => write!(
                f,
                "permission denied: this script may not {access} `{path}`, because its \
                 `permissions` block does not allow it"
            ),
            Self::ProtectedPath { path, reason } => {
                write!(f, "`{path}` can never be written by a script: {reason}")
            }
            Self::MissingPermissionPath(path) => write!(
                f,
                "this permission points to `{path}`, which does not exist"
            ),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}
