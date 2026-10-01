//! Checking values against the types written in the script.

use super::{RunResult, RuntimeErrorKind, error};
use crate::ast::Type;
use crate::token::Span;
use crate::value::Value;

// Until the checker exists, types are checked while the script runs.
pub(super) fn check_type(value: &Value, declared: &Type, span: Span) -> RunResult<()> {
    if conforms(value, declared)? {
        return Ok(());
    }
    Err(error(
        RuntimeErrorKind::TypeMismatch {
            expected: declared.to_string(),
            found: value.type_name(),
        },
        span,
    ))
}

pub(super) fn conforms(value: &Value, declared: &Type) -> RunResult<bool> {
    match (declared.name.as_str(), declared.arguments.as_slice()) {
        ("Int" | "Float" | "Text" | "Bool" | "Json", []) => Ok(value.type_name() == declared.name),
        ("List", [element]) => match value {
            // Lists hold a single type, so the first item speaks for all.
            Value::List(items) => items
                .first()
                .map_or(Ok(true), |first| conforms(first, element)),
            _ => Ok(false),
        },
        ("Optional", [inner]) => match value {
            Value::Present(value) => conforms(value, inner),
            Value::Absent => Ok(true),
            _ => Ok(false),
        },
        ("Result", [success, failure])
            if failure.name == "Error" && failure.arguments.is_empty() =>
        {
            match value {
                Value::Success(inner) => conforms(inner, success),
                Value::Failure(_) => Ok(true),
                _ => Ok(false),
            }
        }
        _ => Err(error(
            RuntimeErrorKind::UnknownType(declared.to_string()),
            declared.span,
        )),
    }
}
