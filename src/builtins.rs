//! The functions every script can call without declaring them, one file per
//! domain, all listed in a single table.

mod io;
mod json;
mod lists;
mod numbers;
mod text;

use std::io::Write;
use std::rc::Rc;

use crate::ast::Expression;
use crate::interpreter::{RunResult, RuntimeError, RuntimeErrorKind, error, type_mismatch};
use crate::json::Json;
use crate::permissions::Permissions;
use crate::token::Span;
use crate::value::Value;

pub(crate) use lists::append_item;

/// What a built-in function may reach outside the script.
pub(crate) struct Context<'a> {
    pub(crate) output: &'a mut dyn Write,
    pub(crate) permissions: &'a Permissions,
}

pub(crate) enum Builtin {
    /// Works only on its arguments: it cannot reach anything outside the script.
    Pure(fn(Vec<Value>, Span) -> RunResult<Value>),
    /// Reaches the screen, the disk or the environment, through `Context`.
    Io(fn(&mut Context<'_>, Vec<Value>, Span) -> RunResult<Value>),
    /// Receives its arguments as written, before they are evaluated.
    Quoted(fn(&Context<'_>, &[Expression], Span) -> RunResult<Value>),
}

const BUILTINS: &[(&str, Builtin)] = &[
    // Outside world
    ("print", Builtin::Io(io::print)),
    ("read_file", Builtin::Io(io::read_file)),
    ("write_file", Builtin::Io(io::write_file)),
    ("list_folder", Builtin::Io(io::list_folder)),
    ("env", Builtin::Quoted(io::env)),
    // Lists
    ("length", Builtin::Pure(lists::length)),
    ("append", Builtin::Pure(lists::append)),
    ("get", Builtin::Pure(lists::get)),
    // Numbers
    ("to_float", Builtin::Pure(numbers::to_float)),
    ("round", Builtin::Pure(numbers::round)),
    ("floor", Builtin::Pure(numbers::floor)),
    ("ceil", Builtin::Pure(numbers::ceil)),
    ("to_text", Builtin::Pure(numbers::to_text)),
    ("parse_int", Builtin::Pure(numbers::parse_int)),
    ("parse_float", Builtin::Pure(numbers::parse_float)),
    ("quotient", Builtin::Pure(numbers::quotient)),
    ("remainder", Builtin::Pure(numbers::remainder)),
    ("format_decimals", Builtin::Pure(numbers::format_decimals)),
    // Text
    ("contains", Builtin::Pure(text::contains)),
    ("starts_with", Builtin::Pure(text::starts_with)),
    ("ends_with", Builtin::Pure(text::ends_with)),
    ("replace_all", Builtin::Pure(text::replace_all)),
    ("split", Builtin::Pure(text::split)),
    ("join", Builtin::Pure(text::join)),
    ("lines", Builtin::Pure(text::lines)),
    ("trim", Builtin::Pure(text::trim)),
    ("to_upper", Builtin::Pure(text::to_upper)),
    ("to_lower", Builtin::Pure(text::to_lower)),
    // JSON
    ("parse_json", Builtin::Pure(json::parse_json)),
    ("field", Builtin::Pure(json::field)),
    ("text_field", Builtin::Pure(json::text_field)),
    ("int_field", Builtin::Pure(json::int_field)),
    ("float_field", Builtin::Pure(json::float_field)),
    ("bool_field", Builtin::Pure(json::bool_field)),
    ("as_text", Builtin::Pure(json::as_text)),
    ("as_int", Builtin::Pure(json::as_int)),
    ("as_float", Builtin::Pure(json::as_float)),
    ("as_bool", Builtin::Pure(json::as_bool)),
    ("as_list", Builtin::Pure(json::as_list)),
    ("is_null", Builtin::Pure(json::is_null)),
    ("has_field", Builtin::Pure(json::has_field)),
];

pub(crate) fn find(name: &str) -> Option<&'static Builtin> {
    BUILTINS
        .iter()
        .find(|(builtin, _)| *builtin == name)
        .map(|(_, builtin)| builtin)
}

pub(crate) fn exists(name: &str) -> bool {
    find(name).is_some()
}

// ----- Arguments -----

fn exact_arguments<const N: usize>(
    function: &str,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<[Value; N]> {
    <[Value; N]>::try_from(arguments).map_err(|arguments| {
        error(
            RuntimeErrorKind::WrongArgumentCount {
                function: function.to_string(),
                expected: N,
                found: arguments.len(),
            },
            span,
        )
    })
}

fn expect_text(value: Value, span: Span) -> RunResult<String> {
    match value {
        Value::Text(text) => Ok(text),
        other => Err(type_mismatch("Text", &other, span)),
    }
}

fn expect_integer(value: Value, span: Span) -> RunResult<i64> {
    match value {
        Value::Integer(integer) => Ok(integer),
        other => Err(type_mismatch("Int", &other, span)),
    }
}

fn expect_float(value: Value, span: Span) -> RunResult<f64> {
    match value {
        Value::Float(float) => Ok(float),
        other => Err(type_mismatch("Float", &other, span)),
    }
}

fn expect_json(value: Value, span: Span) -> RunResult<Rc<Json>> {
    match value {
        Value::Json(json) => Ok(json),
        other => Err(type_mismatch("Json", &other, span)),
    }
}

// ----- Results and errors -----

fn result_value(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => Value::Success(Box::new(value)),
        Err(message) => Value::Failure(message),
    }
}

fn invalid_argument(function: &'static str, reason: &str, span: Span) -> RuntimeError {
    let reason = reason.to_string();
    error(RuntimeErrorKind::InvalidArgument { function, reason }, span)
}
