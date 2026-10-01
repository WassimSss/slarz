//! Evaluating expressions into values.

use std::io::Write;
use std::rc::Rc;

use super::operators::{binary, unary};
use super::{Interpreter, NO_VALUE, RunResult, RuntimeErrorKind, error, not_optional_or_result};
use crate::ast::{BinaryOperator, Expression, ExpressionKind};
use crate::value::Value;

impl<W: Write> Interpreter<'_, W> {
    fn list(&mut self, items: &[Expression]) -> RunResult<Value> {
        let mut values: Vec<Value> = Vec::with_capacity(items.len());
        for item in items {
            let value = self.evaluate(item)?;
            if let Some(first) = values.first() {
                if first.type_name() != value.type_name() {
                    return Err(error(
                        RuntimeErrorKind::TypeMismatch {
                            expected: first.type_name().to_string(),
                            found: value.type_name(),
                        },
                        item.span,
                    ));
                }
            }
            values.push(value);
        }
        Ok(Value::List(Rc::new(values)))
    }

    pub(super) fn evaluate(&mut self, expression: &Expression) -> RunResult<Value> {
        let span = expression.span;
        match &expression.kind {
            ExpressionKind::Integer(value) => Ok(Value::Integer(*value)),
            ExpressionKind::Float(value) => Ok(Value::Float(*value)),
            ExpressionKind::Text(text) => Ok(Value::Text(text.clone())),
            ExpressionKind::Bool(value) => Ok(Value::Bool(*value)),
            ExpressionKind::List(items) => self.list(items),
            ExpressionKind::Variable(name) => self
                .lookup(name)
                .map(|variable| variable.value.clone())
                .ok_or_else(|| error(RuntimeErrorKind::UndefinedVariable(name.clone()), span)),
            ExpressionKind::Unary { operator, operand } => {
                let value = self.evaluate(operand)?;
                unary(*operator, value).map_err(|kind| error(kind, span))
            }
            // `and` and `or` stop as soon as the result is known.
            ExpressionKind::Binary {
                left,
                operator: operator @ (BinaryOperator::And | BinaryOperator::Or),
                right,
            } => {
                let left = self.condition(left)?;
                let decided = match operator {
                    BinaryOperator::And => !left,
                    _ => left,
                };
                if decided {
                    return Ok(Value::Bool(left));
                }
                Ok(Value::Bool(self.condition(right)?))
            }
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;
                binary(*operator, left, right).map_err(|kind| error(kind, span))
            }
            ExpressionKind::Call {
                function,
                arguments,
            } => self.call(function, arguments, span),
            ExpressionKind::Check(inner) => match self.evaluate(inner)? {
                Value::Success(value) | Value::Present(value) => Ok(*value),
                Value::Failure(message) => Err(error(RuntimeErrorKind::Failed(message), span)),
                Value::Absent => Err(error(RuntimeErrorKind::Failed(NO_VALUE.to_string()), span)),
                other => Err(not_optional_or_result(&other, inner.span)),
            },
            ExpressionKind::If {
                condition,
                then_value,
                else_value,
            } => {
                let chosen = if self.condition(condition)? {
                    then_value
                } else {
                    else_value
                };
                self.evaluate(chosen)
            }
            ExpressionKind::Otherwise { value, fallback } => match self.evaluate(value)? {
                Value::Success(value) | Value::Present(value) => Ok(*value),
                Value::Failure(_) | Value::Absent => self.evaluate(fallback),
                other => Err(not_optional_or_result(&other, value.span)),
            },
        }
    }
    pub(super) fn condition(&mut self, expression: &Expression) -> RunResult<bool> {
        match self.evaluate(expression)? {
            Value::Bool(value) => Ok(value),
            other => Err(error(
                RuntimeErrorKind::TypeMismatch {
                    expected: "Bool".to_string(),
                    found: other.type_name(),
                },
                expression.span,
            )),
        }
    }
}
