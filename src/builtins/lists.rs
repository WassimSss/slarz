//! Functions on lists (and `length`, which also counts the characters of a text).

use std::rc::Rc;

use super::{exact_arguments, expect_integer, invalid_argument};
use crate::interpreter::{RunResult, type_mismatch};
use crate::token::Span;
use crate::value::Value;

pub(super) fn length(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = exact_arguments("length", arguments, span)?;
    let length = match &value {
        Value::List(items) => items.len(),
        Value::Text(text) => text.chars().count(),
        other => return Err(type_mismatch("List` or `Text", other, span)),
    };
    Ok(Value::Integer(i64::try_from(length).unwrap_or(i64::MAX)))
}

pub(super) fn append(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [list, item] = exact_arguments("append", arguments, span)?;
    append_item(list, item, span)
}

/// Adds `item` at the end of `list`, copying the list only if another
/// variable still shares it.
pub(crate) fn append_item(list: Value, item: Value, span: Span) -> RunResult<Value> {
    let Value::List(mut items) = list else {
        return Err(type_mismatch("List", &list, span));
    };
    if let Some(first) = items.first() {
        if first.type_name() != item.type_name() {
            return Err(type_mismatch(first.type_name(), &item, span));
        }
    }
    Rc::make_mut(&mut items).push(item);
    Ok(Value::List(items))
}

// Positions start at 0. A negative one is a bug, not an absence: in Python,
// `items[-1]` is the last item, and a script expecting that must not quietly
// get nothing.
pub(super) fn get(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [list, index] = exact_arguments("get", arguments, span)?;
    let Value::List(items) = list else {
        return Err(type_mismatch("List", &list, span));
    };
    let index = expect_integer(index, span)?;
    let Ok(index) = usize::try_from(index) else {
        return Err(invalid_argument(
            "get",
            "positions start at 0 and cannot be negative (there is no `-1` for the last item)",
            span,
        ));
    };
    Ok(match items.get(index) {
        Some(item) => Value::Present(Box::new(item.clone())),
        None => Value::Absent,
    })
}
