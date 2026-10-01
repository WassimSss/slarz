//! Running statements: declarations, assignments, `if`, loops, `return`.

use std::io::Write;

use super::types::check_type;
use super::{Flow, Interpreter, RunResult, RuntimeErrorKind, Scope, error, type_mismatch};
use crate::ast::{Block, Expression, ExpressionKind, Statement, StatementKind};
use crate::builtins;
use crate::value::Value;

impl<W: Write> Interpreter<'_, W> {
    pub(super) fn execute_statements(&mut self, statements: &[Statement]) -> RunResult<Flow> {
        for statement in statements {
            if let Flow::Return(value) = self.execute(statement)? {
                return Ok(Flow::Return(value));
            }
        }
        Ok(Flow::Continue)
    }

    fn execute(&mut self, statement: &Statement) -> RunResult<Flow> {
        let span = statement.span;
        match &statement.kind {
            StatementKind::Declaration {
                name,
                mutable,
                declared_type,
                value,
            } => {
                let value = self.evaluate(value)?;
                check_type(&value, declared_type, span)?;
                self.declare(name, value, *mutable, Some(declared_type.clone()), span)?;
            }
            StatementKind::Assignment { name, value } => {
                let value = match self.append_in_place(name, value)? {
                    Some(value) => value,
                    None => self.evaluate(value)?,
                };
                self.assign(name, value, span)?;
            }
            StatementKind::Expression(expression) => {
                self.evaluate(expression)?;
            }
            StatementKind::If {
                condition,
                then_block,
                else_block,
            } => {
                if self.condition(condition)? {
                    return self.execute_block(then_block);
                }
                if let Some(block) = else_block {
                    return self.execute_block(block);
                }
            }
            StatementKind::IfPresent {
                name,
                declared_type,
                value,
                then_block,
                else_block,
            } => {
                let inner = match self.evaluate(value)? {
                    Value::Present(inner) | Value::Success(inner) => *inner,
                    Value::Absent | Value::Failure(_) => {
                        return match else_block {
                            Some(block) => self.execute_block(block),
                            None => Ok(Flow::Continue),
                        };
                    }
                    other => return Err(type_mismatch("Optional` or `Result", &other, value.span)),
                };
                check_type(&inner, declared_type, span)?;
                // `name` only exists inside the first block.
                self.scopes.push(Scope::new());
                let flow = self
                    .declare(name, inner, false, Some(declared_type.clone()), span)
                    .and_then(|()| self.execute_statements(then_block));
                self.scopes.pop();
                return flow;
            }
            StatementKind::While { condition, body } => {
                while self.condition(condition)? {
                    if let Flow::Return(value) = self.execute_block(body)? {
                        return Ok(Flow::Return(value));
                    }
                }
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                let items = match self.evaluate(iterable)? {
                    Value::List(items) => items,
                    other => {
                        return Err(error(
                            RuntimeErrorKind::TypeMismatch {
                                expected: "List".to_string(),
                                found: other.type_name(),
                            },
                            iterable.span,
                        ));
                    }
                };
                for item in items.iter() {
                    self.scopes.push(Scope::new());
                    let flow = self
                        .declare(variable, item.clone(), false, None, span)
                        .and_then(|()| self.execute_statements(body));
                    self.scopes.pop();
                    if let Flow::Return(value) = flow? {
                        return Ok(Flow::Return(value));
                    }
                }
            }
            StatementKind::Function(_) if self.call_depth == 0 && self.scopes.len() == 1 => {}
            StatementKind::Function(_) => {
                return Err(error(RuntimeErrorKind::NestedFunction, span));
            }
            StatementKind::Return(value) => {
                if self.call_depth == 0 {
                    return Err(error(RuntimeErrorKind::ReturnOutsideFunction, span));
                }
                let value = match value {
                    Some(expression) => self.evaluate(expression)?,
                    None => Value::Nothing,
                };
                return Ok(Flow::Return(value));
            }
        }
        Ok(Flow::Continue)
    }

    fn execute_block(&mut self, block: &Block) -> RunResult<Flow> {
        self.scopes.push(Scope::new());
        let flow = self.execute_statements(block);
        self.scopes.pop();
        flow
    }

    // `x = append(x, item)` replaces `x` anyway, so its list is handed over to
    // `append` instead of being shared with it: no copy, even for a long list.
    // Only when `item` calls no function of the script, since such a function
    // could read or change `x` while its list is lent out.
    fn append_in_place(&mut self, name: &str, value: &Expression) -> RunResult<Option<Value>> {
        let ExpressionKind::Call {
            function,
            arguments,
        } = &value.kind
        else {
            return Ok(None);
        };
        let [list, item] = arguments.as_slice() else {
            return Ok(None);
        };
        let appends_to_itself =
            matches!(&list.kind, ExpressionKind::Variable(source) if source == name);
        let is_mutable_list = self
            .lookup(name)
            .is_some_and(|variable| variable.mutable && matches!(variable.value, Value::List(_)));
        if function != "append"
            || !appends_to_itself
            || !is_mutable_list
            || self.calls_script_function(item)
        {
            return Ok(None);
        }
        let item = self.evaluate(item)?;
        let Some(variable) = self.lookup_mut(name) else {
            return Ok(None);
        };
        let list = std::mem::replace(&mut variable.value, Value::Nothing);
        builtins::append_item(list, item, value.span).map(Some)
    }

    fn calls_script_function(&self, expression: &Expression) -> bool {
        match &expression.kind {
            ExpressionKind::Call {
                function,
                arguments,
            } => {
                self.functions.contains_key(function)
                    || arguments
                        .iter()
                        .any(|argument| self.calls_script_function(argument))
            }
            ExpressionKind::List(items) => {
                items.iter().any(|item| self.calls_script_function(item))
            }
            ExpressionKind::Unary { operand, .. } => self.calls_script_function(operand),
            ExpressionKind::Binary { left, right, .. } => {
                self.calls_script_function(left) || self.calls_script_function(right)
            }
            ExpressionKind::Check(inner) => self.calls_script_function(inner),
            ExpressionKind::If {
                condition,
                then_value,
                else_value,
            } => [condition, then_value, else_value]
                .iter()
                .any(|part| self.calls_script_function(part)),
            ExpressionKind::Otherwise { value, fallback } => {
                self.calls_script_function(value) || self.calls_script_function(fallback)
            }
            ExpressionKind::Integer(_)
            | ExpressionKind::Float(_)
            | ExpressionKind::Text(_)
            | ExpressionKind::Bool(_)
            | ExpressionKind::Variable(_) => false,
        }
    }
}
