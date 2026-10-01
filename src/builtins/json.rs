//! Reading JSON documents. Reading a field gives a `Result`: in an answer
//! from an API, a missing or mistyped field is a surprise with a reason.

use std::rc::Rc;

use super::numbers::is_plain_number;
use super::{exact_arguments, expect_json, expect_text, result_value};
use crate::interpreter::RunResult;
use crate::json::{self, Json};
use crate::token::Span;
use crate::value::Value;

/// What a JSON value is read as.
#[derive(Debug, Clone, Copy)]
enum Target {
    Json,
    Text,
    Int,
    Float,
    Bool,
    List,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Json => "a JSON value",
            Self::Text => "a text",
            Self::Int | Self::Float => "a number",
            Self::Bool => "a boolean",
            Self::List => "a list",
        }
    }
}

pub(super) fn parse_json(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [text] = exact_arguments("parse_json", arguments, span)?;
    let text = expect_text(text, span)?;
    Ok(result_value(
        json::parse(&text)
            .map(|parsed| Value::Json(Rc::new(parsed)))
            .map_err(|error| error.to_string()),
    ))
}

pub(super) fn field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_field("field", Target::Json, arguments, span)
}

pub(super) fn text_field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_field("text_field", Target::Text, arguments, span)
}

pub(super) fn int_field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_field("int_field", Target::Int, arguments, span)
}

pub(super) fn float_field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_field("float_field", Target::Float, arguments, span)
}

pub(super) fn bool_field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_field("bool_field", Target::Bool, arguments, span)
}

pub(super) fn as_text(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_as("as_text", Target::Text, arguments, span)
}

pub(super) fn as_int(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_as("as_int", Target::Int, arguments, span)
}

pub(super) fn as_float(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_as("as_float", Target::Float, arguments, span)
}

pub(super) fn as_bool(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_as("as_bool", Target::Bool, arguments, span)
}

pub(super) fn as_list(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    read_as("as_list", Target::List, arguments, span)
}

fn read_field(
    function: &str,
    target: Target,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [json, key] = exact_arguments(function, arguments, span)?;
    let json = expect_json(json, span)?;
    let key = expect_text(key, span)?;
    let found = match &*json {
        Json::Object(_) => json
            .field(&key)
            .ok_or_else(|| format!("there is no field `{key}`")),
        other => Err(format!(
            "cannot read field `{key}`: the value is {}, not an object",
            other.kind()
        )),
    };
    Ok(result_value(found.and_then(|value| {
        convert(value, target).map_err(|problem| format!("field `{key}` {problem}"))
    })))
}

fn read_as(function: &str, target: Target, arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [json] = exact_arguments(function, arguments, span)?;
    let json = expect_json(json, span)?;
    Ok(result_value(
        convert(&json, target).map_err(|problem| format!("the value {problem}")),
    ))
}

pub(super) fn is_null(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [json] = exact_arguments("is_null", arguments, span)?;
    Ok(Value::Bool(*expect_json(json, span)? == Json::Null))
}

pub(super) fn has_field(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [json, key] = exact_arguments("has_field", arguments, span)?;
    let json = expect_json(json, span)?;
    let key = expect_text(key, span)?;
    Ok(Value::Bool(json.field(&key).is_some()))
}

// Messages say what kind of value was found, never the value itself: they
// end up in logs, and the value may be private.
fn convert(json: &Json, target: Target) -> Result<Value, String> {
    let converted = match (target, json) {
        (Target::Json, json) => Some(Value::Json(Rc::new(json.clone()))),
        (Target::Text, Json::Text(text)) => Some(Value::Text(text.clone())),
        (Target::Bool, Json::Bool(value)) => Some(Value::Bool(*value)),
        (Target::Int, Json::Number(number)) => {
            if !is_plain_number(number, false) {
                return Err("is a number but not a whole one: read it as a `Float`".to_string());
            }
            let integer = number
                .parse()
                .map_err(|_| "is a whole number too large for an `Int`".to_string())?;
            Some(Value::Integer(integer))
        }
        (Target::Float, Json::Number(number)) => {
            let float = number
                .parse::<f64>()
                .ok()
                .filter(|float| float.is_finite())
                .ok_or_else(|| "is a number too large for a `Float`".to_string())?;
            Some(Value::Float(float))
        }
        (Target::List, Json::List(items)) => {
            let items = items
                .iter()
                .map(|item| Value::Json(Rc::new(item.clone())))
                .collect();
            Some(Value::List(Rc::new(items)))
        }
        _ => None,
    };
    converted.ok_or_else(|| format!("is {}, not {}", json.kind(), target.name()))
}
