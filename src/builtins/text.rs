//! Functions on text. The text worked on is always the first argument.

use std::rc::Rc;

use super::{exact_arguments, expect_text, invalid_argument};
use crate::interpreter::{RunResult, type_mismatch};
use crate::token::Span;
use crate::value::Value;

pub(super) fn contains(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    test_text(
        "contains",
        |text, part| text.contains(part),
        arguments,
        span,
    )
}

pub(super) fn starts_with(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    test_text(
        "starts_with",
        |text, start| text.starts_with(start),
        arguments,
        span,
    )
}

pub(super) fn ends_with(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    test_text(
        "ends_with",
        |text, end| text.ends_with(end),
        arguments,
        span,
    )
}

pub(super) fn trim(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    transform_text("trim", |text| text.trim().to_string(), arguments, span)
}

pub(super) fn to_upper(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    transform_text("to_upper", str::to_uppercase, arguments, span)
}

pub(super) fn to_lower(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    transform_text("to_lower", str::to_lowercase, arguments, span)
}

fn test_text(
    function: &str,
    test: fn(&str, &str) -> bool,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [text, part] = exact_arguments(function, arguments, span)?;
    let text = expect_text(text, span)?;
    let part = expect_text(part, span)?;
    Ok(Value::Bool(test(&text, &part)))
}

fn transform_text(
    function: &str,
    transform: fn(&str) -> String,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [text] = exact_arguments(function, arguments, span)?;
    Ok(Value::Text(transform(&expect_text(text, span)?)))
}

pub(super) fn replace_all(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [text, old, new] = exact_arguments("replace_all", arguments, span)?;
    let text = expect_text(text, span)?;
    let old = expect_text(old, span)?;
    let new = expect_text(new, span)?;
    if old.is_empty() {
        return Err(invalid_argument(
            "replace_all",
            "the text to replace cannot be empty",
            span,
        ));
    }
    Ok(Value::Text(text.replace(&old, &new)))
}

// Empty pieces are kept: in `"a,,b"`, the empty column is still a column.
pub(super) fn split(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [text, separator] = exact_arguments("split", arguments, span)?;
    let text = expect_text(text, span)?;
    let separator = expect_text(separator, span)?;
    if separator.is_empty() {
        return Err(invalid_argument(
            "split",
            "the separator cannot be empty",
            span,
        ));
    }
    Ok(text_list(text.split(separator.as_str())))
}

pub(super) fn join(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [list, separator] = exact_arguments("join", arguments, span)?;
    let Value::List(items) = list else {
        return Err(type_mismatch("List<Text>", &list, span));
    };
    let separator = expect_text(separator, span)?;
    let mut texts = Vec::with_capacity(items.len());
    for item in items.iter() {
        match item {
            Value::Text(text) => texts.push(text.as_str()),
            other => return Err(type_mismatch("Text", other, span)),
        }
    }
    Ok(Value::Text(texts.join(&separator)))
}

// `str::lines` accepts both `\n` and `\r\n`, so a file saved on Windows gives
// the same lines everywhere.
pub(super) fn lines(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [text] = exact_arguments("lines", arguments, span)?;
    Ok(text_list(expect_text(text, span)?.lines()))
}

fn text_list<'a>(pieces: impl Iterator<Item = &'a str>) -> Value {
    let list = pieces.map(|piece| Value::Text(piece.to_string())).collect();
    Value::List(Rc::new(list))
}
