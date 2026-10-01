//! The errors that stop a running script, and their messages.

use std::fmt;

use super::MAX_CALL_DEPTH;
use crate::token::Span;
use crate::value::Value;

/// An absence has no reason of its own: the error points at the `check`.
pub(super) const NO_VALUE: &str = "`check` found no value here: give one with `otherwise`, or handle \
                        the absence with `if name: Type = ... { } else { }`";

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeErrorKind {
    UndefinedVariable(String),
    UndefinedFunction(String),
    AlreadyDeclared(String),
    AssignToConstant(String),
    TypeMismatch {
        expected: String,
        found: &'static str,
    },
    UnknownType(String),
    InvalidOperands {
        operator: &'static str,
        left: &'static str,
        right: &'static str,
    },
    InvalidOperand {
        operator: &'static str,
        operand: &'static str,
    },
    IntegerDivision,
    DivisionByZero,
    Overflow,
    InexactConversion {
        value: String,
        target: &'static str,
    },
    /// An argument of the right type, but a value that can only be a bug.
    InvalidArgument {
        function: &'static str,
        reason: String,
    },
    WrongArgumentCount {
        function: String,
        expected: usize,
        found: usize,
    },
    MissingReturn(String),
    UnexpectedReturnValue(String),
    ReturnOutsideFunction,
    NestedFunction,
    TooDeepRecursion,
    NotSupportedYet(&'static str),
    OutputFailed,
    /// The script tried to reach something its `permissions` block does not
    /// allow. It can never be caught: the script stops right away.
    PermissionDenied {
        access: &'static str,
        path: String,
    },
    /// A location no script may ever write, whatever it declares.
    ProtectedPath {
        path: String,
        reason: &'static str,
    },
    MissingPermissionPath(String),
    /// An operation failed and `check` passed the failure up.
    Failed(String),
}

impl RuntimeErrorKind {
    pub fn is_permission_violation(&self) -> bool {
        matches!(
            self,
            Self::PermissionDenied { .. } | Self::ProtectedPath { .. }
        )
    }
}

/// An error that stops a running script, and where it happened.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub span: Span,
}

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

pub(crate) type RunResult<T> = Result<T, RuntimeError>;

pub(crate) fn error(kind: RuntimeErrorKind, span: Span) -> RuntimeError {
    RuntimeError { kind, span }
}

pub(crate) fn type_mismatch(expected: &str, found: &Value, span: Span) -> RuntimeError {
    error(
        RuntimeErrorKind::TypeMismatch {
            expected: expected.to_string(),
            found: found.type_name(),
        },
        span,
    )
}

pub(super) fn not_optional_or_result(value: &Value, span: Span) -> RuntimeError {
    type_mismatch("Optional` or `Result", value, span)
}
