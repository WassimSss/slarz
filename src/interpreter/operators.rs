//! What `+`, `==`, `<`, `not`... do with values.

use std::cmp::Ordering;

use super::RuntimeErrorKind;
use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::Value;

pub(super) fn unary(operator: UnaryOperator, value: Value) -> Result<Value, RuntimeErrorKind> {
    match (operator, value) {
        (UnaryOperator::Negate, Value::Integer(value)) => value
            .checked_neg()
            .map(Value::Integer)
            .ok_or(RuntimeErrorKind::Overflow),
        (UnaryOperator::Negate, Value::Float(value)) => Ok(Value::Float(-value)),
        (UnaryOperator::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
        (operator, value) => Err(RuntimeErrorKind::InvalidOperand {
            operator: match operator {
                UnaryOperator::Negate => "-",
                UnaryOperator::Not => "not",
            },
            operand: value.type_name(),
        }),
    }
}

pub(super) fn binary(
    operator: BinaryOperator,
    left: Value,
    right: Value,
) -> Result<Value, RuntimeErrorKind> {
    use BinaryOperator as Op;
    use Value::{Float, Integer, Text};

    let checked = |result: Option<i64>| result.map(Integer).ok_or(RuntimeErrorKind::Overflow);
    match (operator, left, right) {
        (Op::Add, Integer(a), Integer(b)) => checked(a.checked_add(b)),
        (Op::Subtract, Integer(a), Integer(b)) => checked(a.checked_sub(b)),
        (Op::Multiply, Integer(a), Integer(b)) => checked(a.checked_mul(b)),
        (Op::Divide, Integer(_), Integer(_)) => Err(RuntimeErrorKind::IntegerDivision),
        (Op::Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (Op::Subtract, Float(a), Float(b)) => Ok(Float(a - b)),
        (Op::Multiply, Float(a), Float(b)) => Ok(Float(a * b)),
        (Op::Divide, Float(_), Float(0.0)) => Err(RuntimeErrorKind::DivisionByZero),
        (Op::Divide, Float(a), Float(b)) => Ok(Float(a / b)),
        (Op::Add, Text(a), Text(b)) => Ok(Text(a + &b)),
        (Op::Equal, a, b) if a.type_name() == b.type_name() => Ok(Value::Bool(a == b)),
        (Op::NotEqual, a, b) if a.type_name() == b.type_name() => Ok(Value::Bool(a != b)),
        (op, Integer(a), Integer(b)) if is_ordering(op) => {
            Ok(Value::Bool(ordering_holds(op, a.partial_cmp(&b))))
        }
        (op, Float(a), Float(b)) if is_ordering(op) => {
            Ok(Value::Bool(ordering_holds(op, a.partial_cmp(&b))))
        }
        (operator, left, right) => Err(RuntimeErrorKind::InvalidOperands {
            operator: symbol(operator),
            left: left.type_name(),
            right: right.type_name(),
        }),
    }
}

fn is_ordering(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    )
}

fn ordering_holds(operator: BinaryOperator, ordering: Option<Ordering>) -> bool {
    use Ordering::{Equal, Greater, Less};
    matches!(
        (operator, ordering),
        (BinaryOperator::Less, Some(Less))
            | (BinaryOperator::LessEqual, Some(Less | Equal))
            | (BinaryOperator::Greater, Some(Greater))
            | (BinaryOperator::GreaterEqual, Some(Greater | Equal))
    )
}

fn symbol(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Add => "+",
        BinaryOperator::Subtract => "-",
        BinaryOperator::Multiply => "*",
        BinaryOperator::Divide => "/",
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
        BinaryOperator::Less => "<",
        BinaryOperator::LessEqual => "<=",
        BinaryOperator::Greater => ">",
        BinaryOperator::GreaterEqual => ">=",
        BinaryOperator::And => "and",
        BinaryOperator::Or => "or",
    }
}
