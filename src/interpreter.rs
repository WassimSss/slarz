//! Runs a program by walking its syntax tree.

mod environment;
mod error;
mod expressions;
mod functions;
mod operators;
mod statements;
mod types;

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use crate::ast::{Function, Program};
use crate::permissions::Permissions;
use crate::value::Value;

use crate::diagnostic::NO_VALUE;
use environment::{Scope, Variable};
use error::not_optional_or_result;
pub(crate) use error::{RunResult, error, type_mismatch};
pub use error::{RuntimeError, RuntimeErrorKind};

/// Deep enough for real scripts, shallow enough to stop runaway recursion
/// before the interpreter itself runs out of stack.
pub(crate) const MAX_CALL_DEPTH: usize = 100;

/// Runs `program`, the content of the file at `script`, writing what it
/// prints to `output`. Paths in the script are relative to its folder.
pub fn run(program: &Program, script: &Path, output: &mut impl Write) -> RunResult<()> {
    let permissions = Permissions::new(&program.permissions, script).map_err(|missing| {
        error(
            RuntimeErrorKind::MissingPermissionPath(missing.path),
            missing.span,
        )
    })?;
    let mut interpreter = Interpreter {
        scopes: vec![Scope::new()],
        functions: HashMap::new(),
        call_depth: 0,
        permissions,
        output,
    };
    interpreter.declare_functions(&program.statements)?;
    interpreter.execute_statements(&program.statements)?;
    Ok(())
}

/// What a statement tells the code around it: keep going, or a `return`
/// is travelling up to the function that must stop.
enum Flow {
    Continue,
    Return(Value),
}

struct Interpreter<'o, W: Write> {
    // One scope per block being executed; the innermost is last.
    scopes: Vec<Scope>,
    functions: HashMap<String, Rc<Function>>,
    call_depth: usize,
    permissions: Permissions,
    output: &'o mut W,
}

#[cfg(test)]
mod tests;
