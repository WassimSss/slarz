//! Declaring the script's functions, and calling them or the built-in ones.

use std::io::Write;
use std::rc::Rc;

use super::types::check_type;
use super::{
    Flow, Interpreter, MAX_CALL_DEPTH, RunResult, RuntimeError, RuntimeErrorKind, Scope, Variable,
    error,
};
use crate::ast::{Expression, Function, Statement, StatementKind};
use crate::builtins::{self, Builtin, Context};
use crate::token::Span;
use crate::value::Value;

impl<W: Write> Interpreter<'_, W> {
    // Top-level functions are known before the script starts, so they can
    // be called from anywhere, even above their declaration.
    pub(super) fn declare_functions(&mut self, statements: &[Statement]) -> RunResult<()> {
        for statement in statements {
            if let StatementKind::Function(function) = &statement.kind {
                if builtins::exists(&function.name) || self.functions.contains_key(&function.name) {
                    return Err(error(
                        RuntimeErrorKind::AlreadyDeclared(function.name.clone()),
                        statement.span,
                    ));
                }
                self.functions
                    .insert(function.name.clone(), Rc::new(function.clone()));
            }
        }
        Ok(())
    }

    pub(super) fn call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        span: Span,
    ) -> RunResult<Value> {
        match builtins::find(name) {
            Some(Builtin::Quoted(function)) => return function(&self.context(), arguments, span),
            Some(Builtin::Pure(function)) => {
                let values = self.evaluate_all(arguments)?;
                return function(values, span);
            }
            Some(Builtin::Io(function)) => {
                let values = self.evaluate_all(arguments)?;
                return function(&mut self.context(), values, span);
            }
            None => {}
        }
        let values = self.evaluate_all(arguments)?;
        let function =
            self.functions.get(name).cloned().ok_or_else(|| {
                error(RuntimeErrorKind::UndefinedFunction(name.to_string()), span)
            })?;
        self.call_function(&function, values, span)
    }

    fn evaluate_all(&mut self, expressions: &[Expression]) -> RunResult<Vec<Value>> {
        expressions
            .iter()
            .map(|expression| self.evaluate(expression))
            .collect()
    }

    fn context(&mut self) -> Context<'_> {
        Context {
            output: &mut *self.output,
            permissions: &self.permissions,
        }
    }

    fn call_function(
        &mut self,
        function: &Function,
        arguments: Vec<Value>,
        span: Span,
    ) -> RunResult<Value> {
        if arguments.len() != function.parameters.len() {
            return Err(error(
                RuntimeErrorKind::WrongArgumentCount {
                    function: function.name.clone(),
                    expected: function.parameters.len(),
                    found: arguments.len(),
                },
                span,
            ));
        }
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(error(RuntimeErrorKind::TooDeepRecursion, span));
        }

        let mut scope = Scope::new();
        for (parameter, argument) in function.parameters.iter().zip(arguments) {
            check_type(&argument, &parameter.declared_type, span)?;
            let variable = Variable {
                value: argument,
                mutable: false,
                declared: Some(parameter.declared_type.clone()),
            };
            scope.insert(parameter.name.clone(), variable);
        }

        // A function sees the script's top-level variables and its own
        // parameters, never the local variables of whoever called it.
        let caller_scopes = self.scopes.split_off(1);
        self.scopes.push(scope);
        self.call_depth += 1;
        let flow = self.execute_statements(&function.body);
        self.call_depth -= 1;
        self.scopes.truncate(1);
        self.scopes.extend(caller_scopes);

        // A `check` that fails inside a function returning a `Result` makes
        // the function return that failure to its caller.
        let returns_result = function
            .return_type
            .as_ref()
            .is_some_and(|return_type| return_type.name == "Result");
        let flow = match flow {
            Err(RuntimeError {
                kind: RuntimeErrorKind::Failed(message),
                ..
            }) if returns_result => Ok(Flow::Return(Value::Failure(message))),
            other => other,
        };

        let returned = match flow? {
            Flow::Return(value) => value,
            Flow::Continue => Value::Nothing,
        };
        match (&function.return_type, returned) {
            (Some(_), Value::Nothing) => Err(error(
                RuntimeErrorKind::MissingReturn(function.name.clone()),
                span,
            )),
            (Some(return_type), value) => {
                check_type(&value, return_type, span)?;
                Ok(value)
            }
            (None, Value::Nothing) => Ok(Value::Nothing),
            (None, _) => Err(error(
                RuntimeErrorKind::UnexpectedReturnValue(function.name.clone()),
                span,
            )),
        }
    }
}
