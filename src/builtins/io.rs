//! Functions that reach outside the script: the screen, files and environment
//! variables. Each one checks its permission before touching anything.

use std::rc::Rc;

use super::{Context, exact_arguments, expect_text, invalid_argument};
use crate::ast::{Expression, ExpressionKind};
use crate::interpreter::{RunResult, RuntimeErrorKind, error};
use crate::permissions::Denial;
use crate::token::Span;
use crate::value::Value;

pub(super) fn print(
    context: &mut Context<'_>,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [value] = exact_arguments("print", arguments, span)?;
    writeln!(context.output, "{value}").map_err(|_| error(RuntimeErrorKind::OutputFailed, span))?;
    Ok(Value::Nothing)
}

// Permissions are checked before the disk is touched: a denied path is
// never even looked at.
pub(super) fn read_file(
    context: &mut Context<'_>,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [path] = exact_arguments("read_file", arguments, span)?;
    let path = expect_text(path, span)?;
    let real_path = context
        .permissions
        .resolve_read(&path)
        .map_err(|denial| error(denied("read", path.clone(), denial), span))?;
    Ok(match std::fs::read_to_string(real_path) {
        Ok(content) => Value::Success(Box::new(Value::Text(content))),
        Err(reason) => Value::Failure(format!("cannot read `{path}`: {reason}")),
    })
}

pub(super) fn write_file(
    context: &mut Context<'_>,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [path, content] = exact_arguments("write_file", arguments, span)?;
    let path = expect_text(path, span)?;
    let content = expect_text(content, span)?;
    let real_path = context
        .permissions
        .resolve_write(&path)
        .map_err(|denial| error(denied("write", path.clone(), denial), span))?;
    Ok(match std::fs::write(real_path, content) {
        Ok(()) => Value::Success(Box::new(Value::Nothing)),
        Err(reason) => Value::Failure(format!("cannot write `{path}`: {reason}")),
    })
}

// Paths come back as the script would write them (`./invoices/a.pdf`), in
// alphabetical order so the result is the same on every system; folders
// end with `/`.
pub(super) fn list_folder(
    context: &mut Context<'_>,
    arguments: Vec<Value>,
    span: Span,
) -> RunResult<Value> {
    let [path] = exact_arguments("list_folder", arguments, span)?;
    let path = expect_text(path, span)?;
    let real_path = context
        .permissions
        .resolve_read(&path)
        .map_err(|denial| error(denied("read", path.clone(), denial), span))?;
    let failure =
        |reason: std::io::Error| Value::Failure(format!("cannot list `{path}`: {reason}"));
    let entries = match std::fs::read_dir(real_path) {
        Ok(entries) => entries,
        Err(reason) => return Ok(failure(reason)),
    };
    let prefix = path.trim_end_matches('/');
    let mut paths = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(reason) => return Ok(failure(reason)),
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let suffix = if is_folder { "/" } else { "" };
        paths.push(format!("{prefix}/{name}{suffix}"));
    }
    paths.sort();
    let list = paths.into_iter().map(Value::Text).collect();
    Ok(Value::Success(Box::new(Value::List(Rc::new(list)))))
}

// The name must be written in quotes: a computed name would let outside
// data (an API response, a file) pick which secret to read.
pub(super) fn env(context: &Context<'_>, arguments: &[Expression], span: Span) -> RunResult<Value> {
    let [argument] = arguments else {
        return Err(error(
            RuntimeErrorKind::WrongArgumentCount {
                function: "env".to_string(),
                expected: 1,
                found: arguments.len(),
            },
            span,
        ));
    };
    let ExpressionKind::Text(name) = &argument.kind else {
        return Err(invalid_argument(
            "env",
            "the variable name must be written in quotes, like `env(\"GITHUB_TOKEN\")`, \
             so that anyone reading the script sees which secrets it reads",
            argument.span,
        ));
    };
    if !context.permissions.allows_env(name) {
        return Err(error(
            RuntimeErrorKind::PermissionDenied {
                access: "read the environment variable",
                path: name.clone(),
            },
            span,
        ));
    }
    match std::env::var(name) {
        Ok(value) => Ok(Value::Present(Box::new(Value::Text(value)))),
        Err(std::env::VarError::NotPresent) => Ok(Value::Absent),
        // Saying "absent" would be false: the variable exists.
        Err(std::env::VarError::NotUnicode(_)) => Err(invalid_argument(
            "env",
            &format!("`{name}` is defined but does not hold valid text"),
            span,
        )),
    }
}

fn denied(access: &'static str, path: String, denial: Denial) -> RuntimeErrorKind {
    match denial {
        Denial::NotDeclared => RuntimeErrorKind::PermissionDenied { access, path },
        Denial::Protected(reason) => RuntimeErrorKind::ProtectedPath { path, reason },
    }
}
