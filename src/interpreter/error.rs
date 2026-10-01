//! The errors that stop a running script. Their messages are in `diagnostic`.

use crate::token::Span;
use crate::value::Value;

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
