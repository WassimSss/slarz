//! Conversions between numbers and text, rounding and integer division.
//! Each name says which choice it makes: no conversion hides a decision.

use super::{exact_arguments, expect_float, expect_integer, expect_text, invalid_argument};
use crate::interpreter::{RunResult, RuntimeError, RuntimeErrorKind, error, type_mismatch};
use crate::token::Span;
use crate::value::Value;

/// Beyond 2^53, a `Float` cannot hold every whole number: `to_float` would
/// silently change the value.
const LARGEST_EXACT_FLOAT_INTEGER: i64 = 1 << 53;

/// A `Float` holds about 17 significant digits: more decimals would only
/// print noise.
const MAX_DECIMALS: usize = 17;

pub(super) fn to_float(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = exact_arguments("to_float", arguments, span)?;
    let integer = expect_integer(value, span)?;
    if integer.unsigned_abs() > LARGEST_EXACT_FLOAT_INTEGER.unsigned_abs() {
        return Err(inexact(integer.to_string(), "Float", span));
    }
    Ok(Value::Float(integer as f64))
}

// Halves go away from zero (2.5 gives 3), as taught at school, not to the
// nearest even number like Python.
pub(super) fn round(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    round_with("round", f64::round, arguments, span)
}

pub(super) fn floor(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    round_with("floor", f64::floor, arguments, span)
}

pub(super) fn ceil(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    round_with("ceil", f64::ceil, arguments, span)
}

fn round_with(
    function: &str,
    rounding: fn(f64) -> f64,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [value] = exact_arguments(function, arguments, span)?;
    let float = expect_float(value, span)?;
    let rounded = rounding(float);
    // `i64::MAX as f64` is 2^63, one past the largest `Int`: hence the `<`.
    if !(rounded >= i64::MIN as f64 && rounded < i64::MAX as f64) {
        return Err(inexact(format!("{float:?}"), "Int", span));
    }
    Ok(Value::Integer(rounded as i64))
}

pub(super) fn to_text(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = exact_arguments("to_text", arguments, span)?;
    match value {
        Value::Integer(_) | Value::Float(_) | Value::Bool(_) => Ok(Value::Text(value.to_string())),
        other => Err(type_mismatch("Int`, `Float` or `Bool", &other, span)),
    }
}

pub(super) fn parse_int(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = exact_arguments("parse_int", arguments, span)?;
    let text = expect_text(value, span)?;
    let parsed = is_plain_number(&text, false)
        .then(|| text.parse::<i64>().ok())
        .flatten();
    Ok(match parsed {
        Some(integer) => Value::Success(Box::new(Value::Integer(integer))),
        None => Value::Failure(format!(
            "`{text}` is not a whole number that fits in an `Int`"
        )),
    })
}

pub(super) fn parse_float(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = exact_arguments("parse_float", arguments, span)?;
    let text = expect_text(value, span)?;
    let parsed = is_plain_number(&text, true)
        .then(|| text.parse::<f64>().ok())
        .flatten()
        .filter(|float| float.is_finite());
    Ok(match parsed {
        Some(float) => Value::Success(Box::new(Value::Float(float))),
        None => Value::Failure(format!("`{text}` is not a number like `42` or `3.14`")),
    })
}

/// Only digits, an optional leading `-` and, for decimals, one `.` with
/// digits on both sides. Rust's own parsing is more lenient (`+1`, `1e5`,
/// `inf`): a value read from a file should not be accepted in a shape the
/// script never planned for.
pub(super) fn is_plain_number(text: &str, allow_fraction: bool) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match unsigned.split_once('.') {
        Some((whole, fraction)) => allow_fraction && all_digits(whole) && all_digits(fraction),
        None => all_digits(unsigned),
    }
}

// Both truncate toward zero, like C, Java, JavaScript, Rust and Go:
// `quotient(-7, 2)` is -3 and `remainder(-7, 2)` is -1.
pub(super) fn quotient(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    divide_integers("quotient", i64::checked_div, arguments, span)
}

pub(super) fn remainder(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    divide_integers("remainder", i64::checked_rem, arguments, span)
}

fn divide_integers(
    function: &str,
    operation: fn(i64, i64) -> Option<i64>,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [left, right] = exact_arguments(function, arguments, span)?;
    let left = expect_integer(left, span)?;
    let right = expect_integer(right, span)?;
    match operation(left, right) {
        Some(result) => Ok(Value::Integer(result)),
        None if right == 0 => Err(error(RuntimeErrorKind::DivisionByZero, span)),
        // The only other case: `i64::MIN` divided by -1.
        None => Err(error(RuntimeErrorKind::Overflow, span)),
    }
}

// Halves are rounded away from zero, like `round`: Rust's own formatting
// would round them to even (2.5 to "2"). Values that are not exact in binary
// keep their trap: 1.005 * 100 is 100.49999999999999, so 1.005 gives "1.00".
// Amounts of money belong in `Int` cents, not in `Float`.
pub(super) fn format_decimals(arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [number, decimals] = exact_arguments("format_decimals", arguments, span)?;
    let number = expect_float(number, span)?;
    let decimals = expect_integer(decimals, span)?;
    // A negative count fails `try_from`.
    let Some(decimals) = usize::try_from(decimals)
        .ok()
        .filter(|&decimals| decimals <= MAX_DECIMALS)
    else {
        return Err(invalid_argument(
            "format_decimals",
            &format!("the number of decimals must be between 0 and {MAX_DECIMALS}"),
            span,
        ));
    };
    let factor = 10_f64.powi(decimals as i32);
    let rounded = (number * factor).round() / factor;
    // Huge numbers overflow once scaled, but they have no decimals to round.
    let rounded = if rounded.is_finite() { rounded } else { number };
    Ok(Value::Text(format!("{rounded:.decimals$}")))
}

fn inexact(value: String, target: &'static str, span: Span) -> RuntimeError {
    error(RuntimeErrorKind::InexactConversion { value, target }, span)
}
