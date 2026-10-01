//! Variables and the scopes that hold them.

use std::collections::HashMap;
use std::io::Write;

use super::types::conforms;
use super::{Interpreter, RunResult, RuntimeErrorKind, error};
use crate::ast::Type;
use crate::token::Span;
use crate::value::Value;

pub(super) struct Variable {
    pub(super) value: Value,
    pub(super) mutable: bool,
    // What a later assignment must match. Loop variables have none: they
    // are constants, so they are never assigned.
    pub(super) declared: Option<Type>,
}

pub(super) type Scope = HashMap<String, Variable>;

impl<W: Write> Interpreter<'_, W> {
    pub(super) fn lookup(&self, name: &str) -> Option<&Variable> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    pub(super) fn lookup_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
    }

    pub(super) fn declare(
        &mut self,
        name: &str,
        value: Value,
        mutable: bool,
        declared: Option<Type>,
        span: Span,
    ) -> RunResult<()> {
        if self.lookup(name).is_some() {
            return Err(error(
                RuntimeErrorKind::AlreadyDeclared(name.to_string()),
                span,
            ));
        }
        if let Some(scope) = self.scopes.last_mut() {
            let variable = Variable {
                value,
                mutable,
                declared,
            };
            scope.insert(name.to_string(), variable);
        }
        Ok(())
    }

    pub(super) fn assign(&mut self, name: &str, value: Value, span: Span) -> RunResult<()> {
        let variable = self
            .lookup_mut(name)
            .ok_or_else(|| error(RuntimeErrorKind::UndefinedVariable(name.to_string()), span))?;
        if !variable.mutable {
            return Err(error(
                RuntimeErrorKind::AssignToConstant(name.to_string()),
                span,
            ));
        }
        let matches = match &variable.declared {
            Some(declared) => conforms(&value, declared)?,
            None => variable.value.type_name() == value.type_name(),
        };
        if !matches {
            let expected = variable
                .declared
                .as_ref()
                .map_or_else(|| variable.value.type_name().to_string(), Type::to_string);
            return Err(error(
                RuntimeErrorKind::TypeMismatch {
                    expected,
                    found: value.type_name(),
                },
                span,
            ));
        }
        variable.value = value;
        Ok(())
    }
}
