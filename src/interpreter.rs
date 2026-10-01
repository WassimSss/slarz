//! Runs a program by walking its syntax tree.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use crate::ast::{
    BinaryOperator, Block, Expression, ExpressionKind, Function, Program, Statement, StatementKind,
    Type, UnaryOperator,
};
use crate::builtins::{self, Builtin, Context};
use crate::permissions::Permissions;
use crate::token::Span;
use crate::value::Value;

/// Deep enough for real scripts, shallow enough to stop runaway recursion
/// before the interpreter itself runs out of stack.
const MAX_CALL_DEPTH: usize = 100;

/// An absence has no reason of its own: the error points at the `check`.
const NO_VALUE: &str = "`check` found no value here: give one with `otherwise`, or handle \
                        the absence with `if name: Type = ... { } else { }`";

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeErrorKind {
    UndefinedVariable(String),
    UndefinedFunction(String),
    AlreadyDeclared(String),
    AssignToConstant(String),
    TypeMismatch {
        expected: String,
        found: &'static str,
    },
    UnknownType(String),
    InvalidOperands {
        operator: &'static str,
        left: &'static str,
        right: &'static str,
    },
    InvalidOperand {
        operator: &'static str,
        operand: &'static str,
    },
    IntegerDivision,
    DivisionByZero,
    Overflow,
    InexactConversion {
        value: String,
        target: &'static str,
    },
    /// An argument of the right type, but a value that can only be a bug.
    InvalidArgument {
        function: &'static str,
        reason: String,
    },
    WrongArgumentCount {
        function: String,
        expected: usize,
        found: usize,
    },
    MissingReturn(String),
    UnexpectedReturnValue(String),
    ReturnOutsideFunction,
    NestedFunction,
    TooDeepRecursion,
    NotSupportedYet(&'static str),
    OutputFailed,
    /// The script tried to reach something its `permissions` block does not
    /// allow. It can never be caught: the script stops right away.
    PermissionDenied {
        access: &'static str,
        path: String,
    },
    /// A location no script may ever write, whatever it declares.
    ProtectedPath {
        path: String,
        reason: &'static str,
    },
    MissingPermissionPath(String),
    /// An operation failed and `check` passed the failure up.
    Failed(String),
}

impl RuntimeErrorKind {
    pub fn is_permission_violation(&self) -> bool {
        matches!(
            self,
            Self::PermissionDenied { .. } | Self::ProtectedPath { .. }
        )
    }
}

/// An error that stops a running script, and where it happened.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub span: Span,
}

impl fmt::Display for RuntimeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedVariable(name) => write!(
                f,
                "`{name}` is not defined: declare it first with `{name}: Type = value;`"
            ),
            Self::UndefinedFunction(name) => write!(f, "there is no function called `{name}`"),
            Self::AlreadyDeclared(name) => write!(
                f,
                "`{name}` already exists: a name cannot be declared twice, pick another one"
            ),
            Self::AssignToConstant(name) => write!(
                f,
                "`{name}` is a constant: declare it with `var` if it needs to change"
            ),
            Self::TypeMismatch { expected, found } => {
                write!(f, "expected a value of type `{expected}`, found `{found}`")
            }
            Self::UnknownType(name) if name == "String" || name == "str" => {
                write!(f, "unknown type `{name}`: the text type is called `Text`")
            }
            Self::UnknownType(name) => write!(
                f,
                "unknown type `{name}`: the available types are `Int`, `Float`, `Text`, `Bool`, \
                 `Json`, `List<T>`, `Optional<T>` and `Result<T, Error>`"
            ),
            Self::InvalidOperands {
                operator,
                left,
                right,
            } => write!(
                f,
                "`{operator}` cannot be used between `{left}` and `{right}`"
            ),
            Self::InvalidOperand { operator, operand } => {
                write!(f, "`{operator}` cannot be used on `{operand}`")
            }
            Self::IntegerDivision => write!(
                f,
                "`/` only divides `Float` values, because languages disagree on what `7 / 2` \
                 means for integers: write `quotient(7, 2)` for a whole number, or `7.0 / 2.0`"
            ),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::Overflow => write!(f, "integer overflow: the result does not fit in an `Int`"),
            Self::InexactConversion { value, target } => write!(
                f,
                "`{value}` cannot be turned into a `{target}` without changing its value"
            ),
            Self::InvalidArgument { function, reason } => write!(f, "`{function}`: {reason}"),
            Self::WrongArgumentCount {
                function,
                expected,
                found,
            } => write!(
                f,
                "`{function}` expects {expected} argument(s), but got {found}"
            ),
            Self::MissingReturn(name) => write!(
                f,
                "function `{name}` ended without returning a value: add a `return` statement"
            ),
            Self::UnexpectedReturnValue(name) => write!(
                f,
                "function `{name}` has no return type, so it cannot return a value: \
                 add `-> Type` to its declaration"
            ),
            Self::ReturnOutsideFunction => write!(f, "`return` can only be used inside a function"),
            Self::NestedFunction => write!(
                f,
                "functions must be declared at the top level of the script, not inside a block"
            ),
            Self::TooDeepRecursion => write!(
                f,
                "too many nested function calls (the limit is {MAX_CALL_DEPTH})"
            ),
            Self::NotSupportedYet(what) => write!(f, "{what}"),
            Self::OutputFailed => write!(f, "could not write the output"),
            Self::PermissionDenied { access, path } => write!(
                f,
                "permission denied: this script may not {access} `{path}`, because its \
                 `permissions` block does not allow it"
            ),
            Self::ProtectedPath { path, reason } => {
                write!(f, "`{path}` can never be written by a script: {reason}")
            }
            Self::MissingPermissionPath(path) => write!(
                f,
                "this permission points to `{path}`, which does not exist"
            ),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

pub(crate) type RunResult<T> = Result<T, RuntimeError>;

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

struct Variable {
    value: Value,
    mutable: bool,
    // What a later assignment must match. Loop variables have none: they
    // are constants, so they are never assigned.
    declared: Option<Type>,
}

type Scope = HashMap<String, Variable>;

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

impl<W: Write> Interpreter<'_, W> {
    // Top-level functions are known before the script starts, so they can
    // be called from anywhere, even above their declaration.
    fn declare_functions(&mut self, statements: &[Statement]) -> RunResult<()> {
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

    // ----- Statements -----

    fn execute_statements(&mut self, statements: &[Statement]) -> RunResult<Flow> {
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

    fn condition(&mut self, expression: &Expression) -> RunResult<bool> {
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

    // ----- Variables -----

    fn lookup(&self, name: &str) -> Option<&Variable> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
    }

    fn declare(
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

    fn assign(&mut self, name: &str, value: Value, span: Span) -> RunResult<()> {
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

    // ----- Expressions -----

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

    fn evaluate(&mut self, expression: &Expression) -> RunResult<Value> {
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

    fn call(&mut self, name: &str, arguments: &[Expression], span: Span) -> RunResult<Value> {
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

pub(crate) fn error(kind: RuntimeErrorKind, span: Span) -> RuntimeError {
    RuntimeError { kind, span }
}

pub(crate) fn type_mismatch(expected: &str, found: &Value, span: Span) -> RuntimeError {
    error(
        RuntimeErrorKind::TypeMismatch {
            expected: expected.to_string(),
            found: found.type_name(),
        },
        span,
    )
}

fn not_optional_or_result(value: &Value, span: Span) -> RuntimeError {
    type_mismatch("Optional` or `Result", value, span)
}

// Until the checker exists, types are checked while the script runs.
fn check_type(value: &Value, declared: &Type, span: Span) -> RunResult<()> {
    if conforms(value, declared)? {
        return Ok(());
    }
    Err(error(
        RuntimeErrorKind::TypeMismatch {
            expected: declared.to_string(),
            found: value.type_name(),
        },
        span,
    ))
}

fn conforms(value: &Value, declared: &Type) -> RunResult<bool> {
    match (declared.name.as_str(), declared.arguments.as_slice()) {
        ("Int" | "Float" | "Text" | "Bool" | "Json", []) => Ok(value.type_name() == declared.name),
        ("List", [element]) => match value {
            // Lists hold a single type, so the first item speaks for all.
            Value::List(items) => items
                .first()
                .map_or(Ok(true), |first| conforms(first, element)),
            _ => Ok(false),
        },
        ("Optional", [inner]) => match value {
            Value::Present(value) => conforms(value, inner),
            Value::Absent => Ok(true),
            _ => Ok(false),
        },
        ("Result", [success, failure])
            if failure.name == "Error" && failure.arguments.is_empty() =>
        {
            match value {
                Value::Success(inner) => conforms(inner, success),
                Value::Failure(_) => Ok(true),
                _ => Ok(false),
            }
        }
        _ => Err(error(
            RuntimeErrorKind::UnknownType(declared.to_string()),
            declared.span,
        )),
    }
}

fn unary(operator: UnaryOperator, value: Value) -> Result<Value, RuntimeErrorKind> {
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

fn binary(operator: BinaryOperator, left: Value, right: Value) -> Result<Value, RuntimeErrorKind> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn program(body: &str) -> Program {
        parse(tokenize(&format!("permissions {{ }}\n{body}")).unwrap()).unwrap()
    }

    fn output(body: &str) -> String {
        let mut output = Vec::new();
        run(&program(body), Path::new("script.slz"), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn runtime_error(body: &str) -> RuntimeErrorKind {
        run(&program(body), Path::new("script.slz"), &mut Vec::new())
            .unwrap_err()
            .kind
    }

    /// A throwaway folder holding a small set of files, next to a script:
    /// `data/notes.txt`, `data/2025/january.txt`, `database.txt`, `secret.txt`.
    fn fixture(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("slarz-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("data/2025")).unwrap();
        std::fs::write(folder.join("data/notes.txt"), "notes").unwrap();
        std::fs::write(folder.join("data/2025/january.txt"), "january").unwrap();
        std::fs::write(folder.join("database.txt"), "database").unwrap();
        std::fs::write(folder.join("secret.txt"), "secret").unwrap();
        folder
    }

    fn run_in(folder: &Path, source: &str) -> Result<String, RuntimeErrorKind> {
        let program = parse(tokenize(source).unwrap()).unwrap();
        let mut output = Vec::new();
        run(&program, &folder.join("script.slz"), &mut output).map_err(|error| error.kind)?;
        Ok(String::from_utf8(output).unwrap())
    }

    fn read_in_data(folder: &Path, path: &str) -> Result<String, RuntimeErrorKind> {
        let source = format!(
            "permissions {{ read folder \"./data\"; }}\nprint(check read_file(\"{path}\"));"
        );
        run_in(folder, &source)
    }

    fn not_declared(access: &'static str, path: &str) -> RuntimeErrorKind {
        RuntimeErrorKind::PermissionDenied {
            access,
            path: path.to_string(),
        }
    }

    #[test]
    fn reads_inside_a_declared_folder() {
        let folder = fixture("inside");
        assert_eq!(
            read_in_data(&folder, "./data/notes.txt"),
            Ok("notes\n".into())
        );
        assert_eq!(
            read_in_data(&folder, "./data/2025/january.txt"),
            Ok("january\n".into())
        );
        assert_eq!(
            read_in_data(&folder, "./data/2025/../notes.txt"),
            Ok("notes\n".into())
        );
    }

    #[test]
    fn refuses_anything_outside() {
        let folder = fixture("outside");
        for path in [
            "./database.txt",
            "./data/../secret.txt",
            "./data/missing/../../secret.txt",
            "../secret.txt",
        ] {
            assert_eq!(
                read_in_data(&folder, path),
                Err(not_declared("read", path)),
                "{path}"
            );
        }
    }

    #[test]
    fn a_missing_file_is_a_normal_failure_not_a_violation() {
        let folder = fixture("missing");
        let error = read_in_data(&folder, "./data/nothing.txt").unwrap_err();
        assert!(matches!(error, RuntimeErrorKind::Failed(_)));
        assert!(!error.is_permission_violation());
    }

    #[test]
    fn a_declared_file_allows_only_that_file() {
        let folder = fixture("file");
        let source = |path: &str| {
            format!(
                "permissions {{ read file \"./secret.txt\"; }}\nprint(check read_file(\"{path}\"));"
            )
        };
        assert_eq!(
            run_in(&folder, &source("./secret.txt")),
            Ok("secret\n".into())
        );
        assert_eq!(
            run_in(&folder, &source("./database.txt")),
            Err(not_declared("read", "./database.txt"))
        );
    }

    fn write_in_out(folder: &Path, path: &str) -> Result<String, RuntimeErrorKind> {
        std::fs::create_dir_all(folder.join("out")).unwrap();
        let source = format!(
            "permissions {{ write folder \"./out\"; write folder \".\"; }}\n\
             check write_file(\"{path}\", \"written\");"
        );
        run_in(folder, &source)
    }

    #[test]
    fn writes_inside_a_declared_folder() {
        let folder = fixture("write");
        std::fs::create_dir_all(folder.join("out")).unwrap();
        let source = "permissions { write folder \"./out\"; }
            check write_file(\"./out/report.txt\", \"total: 3\");";
        assert_eq!(run_in(&folder, source), Ok(String::new()));
        assert_eq!(
            std::fs::read_to_string(folder.join("out/report.txt")).unwrap(),
            "total: 3"
        );
        let outside = "permissions { write folder \"./out\"; }
            check write_file(\"./out/../secret.txt\", \"x\");";
        assert_eq!(
            run_in(&folder, outside),
            Err(not_declared("write", "./out/../secret.txt"))
        );
    }

    #[test]
    fn writing_does_not_allow_reading() {
        let folder = fixture("write-read");
        std::fs::create_dir_all(folder.join("out")).unwrap();
        std::fs::write(folder.join("out/old.txt"), "old").unwrap();
        let source = "permissions { write folder \"./out\"; }
            print(check read_file(\"./out/old.txt\"));";
        assert_eq!(
            run_in(&folder, source),
            Err(not_declared("read", "./out/old.txt"))
        );
    }

    #[test]
    fn some_places_can_never_be_written() {
        let folder = fixture("protected");
        for path in [
            "./script.slz",
            "./.git/hooks/pre-commit",
            "./.bashrc",
            "./out/../.zshrc",
            "./Microsoft.PowerShell_profile.ps1",
        ] {
            let error = write_in_out(&folder, path).unwrap_err();
            assert!(
                matches!(error, RuntimeErrorKind::ProtectedPath { .. }),
                "{path}: {error:?}"
            );
            assert!(error.is_permission_violation());
        }
    }

    #[test]
    fn declared_paths_must_exist() {
        let folder = fixture("declared");
        assert_eq!(
            run_in(&folder, "permissions { read folder \"./nowhere\"; }"),
            Err(RuntimeErrorKind::MissingPermissionPath(
                "./nowhere".to_string()
            ))
        );
    }

    #[test]
    fn results_and_check() {
        let folder = fixture("results");
        let script = "permissions { read folder \"./data\"; }
            ok: Result<Text, Error> = read_file(\"./data/notes.txt\");
            print(ok);
            function load() -> Result<Text, Error> {
                text: Text = check read_file(\"./data/nothing.txt\");
                return read_file(\"./data/notes.txt\");
            }
            failed: Result<Text, Error> = load();
            print(failed);";
        let output = run_in(&folder, script).unwrap();
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some("Ok(notes)"));
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("Error(cannot read `./data/nothing.txt`")
        );
        assert_eq!(
            runtime_error("x: Int = check 5;"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Optional` or `Result".to_string(),
                found: "Int",
            }
        );
    }

    #[test]
    fn otherwise_replaces_failures_only() {
        let folder = fixture("otherwise");
        let script = "permissions { read folder \"./data\"; }
            found: Text = read_file(\"./data/notes.txt\") otherwise \"default\";
            print(found);
            missing: Text = read_file(\"./data/nothing.txt\") otherwise \"default\";
            print(missing);";
        assert_eq!(run_in(&folder, script).unwrap(), "notes\ndefault\n");
        assert_eq!(
            runtime_error("x: Int = 5 otherwise 6;"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Optional` or `Result".to_string(),
                found: "Int",
            }
        );
    }

    #[test]
    fn otherwise_evaluates_the_fallback_only_on_failure() {
        let folder = fixture("otherwise-lazy");
        // Reading outside the permissions would stop the script: it must not happen.
        let script = "permissions { read folder \"./data\"; }
            text: Text = read_file(\"./data/notes.txt\") otherwise check read_file(\"../secret.txt\");
            print(text);";
        assert_eq!(run_in(&folder, script).unwrap(), "notes\n");
    }

    const USER: &str = r#"user: Json = check parse_json("{\"login\": \"ada\", \"id\": 1234567890123456789, \"score\": 2.5, \"admin\": false, \"company\": null, \"repos\": [{\"name\": \"slarz\"}]}");"#;

    fn json_output(body: &str) -> String {
        output(&format!("{USER} {body}"))
    }

    fn json_failure(body: &str) -> String {
        let printed = json_output(&format!("print({body});"));
        printed
            .strip_prefix("Error(")
            .and_then(|rest| rest.strip_suffix(")\n"))
            .unwrap_or(&printed)
            .to_string()
    }

    #[test]
    fn json_fields() {
        assert_eq!(
            json_output("print(check text_field(user, \"login\"));"),
            "ada\n"
        );
        assert_eq!(
            json_output("print(check int_field(user, \"id\"));"),
            "1234567890123456789\n"
        );
        assert_eq!(
            json_output("print(check float_field(user, \"score\"));"),
            "2.5\n"
        );
        assert_eq!(
            json_output("print(check bool_field(user, \"admin\"));"),
            "false\n"
        );
        assert_eq!(
            json_output("print(text_field(user, \"company\") otherwise \"none\");"),
            "none\n"
        );
        assert_eq!(
            json_output(
                "for repo in check as_list(check field(user, \"repos\")) {
                     print(check text_field(repo, \"name\"));
                 }"
            ),
            "slarz\n"
        );
    }

    #[test]
    fn json_failures_explain_what_was_found() {
        assert_eq!(
            json_failure("text_field(user, \"email\")"),
            "there is no field `email`"
        );
        assert_eq!(
            json_failure("text_field(user, \"id\")"),
            "field `id` is a number, not a text"
        );
        assert_eq!(
            json_failure("int_field(user, \"score\")"),
            "field `score` is a number but not a whole one: read it as a `Float`"
        );
        assert_eq!(
            json_failure("text_field(user, \"company\")"),
            "field `company` is null, not a text"
        );
        assert_eq!(
            json_failure("as_text(check field(user, \"repos\"))"),
            "the value is a list, not a text"
        );
        assert_eq!(
            json_failure("field(check field(user, \"repos\"), \"name\")"),
            "cannot read field `name`: the value is a list, not an object"
        );
        assert_eq!(
            output("print(parse_json(\"{\\\"a\\\": 1, \\\"a\\\": 2}\"));"),
            "Error(invalid JSON at line 1, column 10: the key \"a\" appears twice)\n"
        );
    }

    #[test]
    fn json_absent_and_null_differ() {
        assert_eq!(
            json_output(
                "print(has_field(user, \"company\"));
                 print(is_null(check field(user, \"company\")));
                 print(has_field(user, \"email\"));"
            ),
            "true\ntrue\nfalse\n"
        );
    }

    #[test]
    fn json_prints_as_compact_json() {
        assert_eq!(
            json_output("print(check field(user, \"repos\"));"),
            "[{\"name\":\"slarz\"}]\n"
        );
    }

    #[test]
    fn get_counts_from_zero() {
        let list = "items: List<Text> = [\"a\", \"b\"];";
        assert_eq!(
            output(&format!("{list} print(get(items, 0));")),
            "Present(a)\n"
        );
        assert_eq!(output(&format!("{list} print(get(items, 2));")), "Absent\n");
        assert!(matches!(
            runtime_error(&format!("{list} x: Optional<Text> = get(items, -1);")),
            RuntimeErrorKind::InvalidArgument {
                function: "get",
                ..
            }
        ));
        assert_eq!(
            runtime_error(&format!("{list} x: Optional<Int> = get(items, 0);")),
            RuntimeErrorKind::TypeMismatch {
                expected: "Optional<Int>".to_string(),
                found: "Optional",
            }
        );
    }

    fn run_with_header(source: &str) -> Result<String, RuntimeErrorKind> {
        let program = parse(tokenize(source).unwrap()).unwrap();
        let mut output = Vec::new();
        run(&program, Path::new("script.slz"), &mut output).map_err(|error| error.kind)?;
        Ok(String::from_utf8(output).unwrap())
    }

    #[test]
    fn env_reads_only_declared_names() {
        let missing = "SLARZ_TEST_VARIABLE_THAT_IS_NEVER_DEFINED";
        assert_eq!(
            run_with_header(&format!(
                "permissions {{ env \"{missing}\"; }} print(env(\"{missing}\"));"
            )),
            Ok("Absent\n".to_string())
        );
        // A loop trying names stops at the first undeclared one: the
        // violation cannot be caught by `otherwise`.
        let denied = run_with_header(
            "permissions { env \"GITHUB_TOKEN\"; }
             x: Text = env(\"AWS_SECRET_KEY\") otherwise \"\";",
        )
        .unwrap_err();
        assert_eq!(
            denied,
            RuntimeErrorKind::PermissionDenied {
                access: "read the environment variable",
                path: "AWS_SECRET_KEY".to_string(),
            }
        );
        assert!(denied.is_permission_violation());
        // Names are compared exactly, whatever the system.
        assert!(matches!(
            run_with_header("permissions { env \"Path\"; } x: Optional<Text> = env(\"PATH\");"),
            Err(RuntimeErrorKind::PermissionDenied { .. })
        ));
    }

    #[test]
    fn env_needs_a_name_written_in_quotes() {
        let error = run_with_header(
            "permissions { env \"GITHUB_TOKEN\"; }
             name: Text = \"GITHUB_TOKEN\";
             x: Optional<Text> = env(name);",
        );
        assert!(matches!(
            error,
            Err(RuntimeErrorKind::InvalidArgument {
                function: "env",
                ..
            })
        ));
    }

    #[test]
    fn optional_with_check_and_otherwise() {
        let list = "items: List<Int> = [10, 20];";
        assert_eq!(
            output(&format!("{list} print(check get(items, 1));")),
            "20\n"
        );
        assert_eq!(
            output(&format!("{list} print(get(items, 5) otherwise 0);")),
            "0\n"
        );
        assert_eq!(
            runtime_error(&format!("{list} x: Int = check get(items, 5);")),
            RuntimeErrorKind::Failed(NO_VALUE.to_string())
        );
    }

    #[test]
    fn if_present_binds_only_inside_its_block() {
        let body = "items: List<Text> = [\"a\", \"b\"];
            if second: Text = get(items, 1) { print(second); } else { print(\"none\"); }
            if third: Text = get(items, 2) { print(third); } else { print(\"none\"); }";
        assert_eq!(output(body), "b\nnone\n");
        assert_eq!(
            runtime_error(
                "items: List<Text> = [\"a\"];
                 if first: Text = get(items, 0) { } print(first);"
            ),
            RuntimeErrorKind::UndefinedVariable("first".to_string())
        );
        assert!(matches!(
            runtime_error("if x: Int = 5 { }"),
            RuntimeErrorKind::TypeMismatch { found: "Int", .. }
        ));
    }

    #[test]
    fn arithmetic_and_print() {
        assert_eq!(output("print(1 + 2 * 3);"), "7\n");
        assert_eq!(output("print(7.0 / 2.0);"), "3.5\n");
        assert_eq!(output("print(3.0);"), "3.0\n");
        assert_eq!(output("print(\"Hello, \" + \"world\");"), "Hello, world\n");
        assert_eq!(output("print(-(2 - 5));"), "3\n");
    }

    #[test]
    fn number_conversions() {
        assert_eq!(output("print(to_float(3));"), "3.0\n");
        assert_eq!(output("print(round(2.5));"), "3\n");
        assert_eq!(output("print(round(-2.5));"), "-3\n");
        assert_eq!(output("print(floor(-2.5));"), "-3\n");
        assert_eq!(output("print(ceil(2.1));"), "3\n");
        assert_eq!(output("print(\"total: \" + to_text(3.0));"), "total: 3.0\n");
        assert_eq!(output("print(to_text(true));"), "true\n");
        assert_eq!(
            runtime_error("x: Float = to_float(9007199254740993);"),
            RuntimeErrorKind::InexactConversion {
                value: "9007199254740993".to_string(),
                target: "Float",
            }
        );
        assert!(matches!(
            runtime_error("x: Int = round(10000000000000000000.0);"),
            RuntimeErrorKind::InexactConversion { target: "Int", .. }
        ));
        assert_eq!(
            runtime_error("x: Int = round(3);"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Float".to_string(),
                found: "Int",
            }
        );
    }

    #[test]
    fn parsing_numbers_is_strict() {
        assert_eq!(output("print(parse_int(\"-42\"));"), "Ok(-42)\n");
        assert_eq!(output("print(parse_float(\"3.14\"));"), "Ok(3.14)\n");
        assert_eq!(output("print(parse_float(\"42\"));"), "Ok(42.0)\n");
        for rejected in [
            "\" 42\"",
            "\"+42\"",
            "\"4.2\"",
            "\"\"",
            "\"99999999999999999999\"",
        ] {
            let body = format!("print(parse_int({rejected}) otherwise 0);");
            assert_eq!(output(&body), "0\n", "parse_int({rejected})");
        }
        for rejected in ["\"1,5\"", "\"1e5\"", "\"inf\"", "\".5\"", "\"5.\"", "\"-\""] {
            let body = format!("print(parse_float({rejected}) otherwise 0.0);");
            assert_eq!(output(&body), "0.0\n", "parse_float({rejected})");
        }
    }

    #[test]
    fn integer_division_truncates_toward_zero() {
        assert_eq!(output("print(quotient(7, 2));"), "3\n");
        assert_eq!(output("print(quotient(-7, 2));"), "-3\n");
        assert_eq!(output("print(remainder(-7, 2));"), "-1\n");
        assert_eq!(
            runtime_error("x: Int = remainder(7, 0);"),
            RuntimeErrorKind::DivisionByZero
        );
        assert_eq!(
            runtime_error("x: Int = quotient(-9223372036854775807 - 1, -1);"),
            RuntimeErrorKind::Overflow
        );
    }

    #[test]
    fn text_tests() {
        assert_eq!(
            output("print(contains(\"invoice-03.pdf\", \"03\"));"),
            "true\n"
        );
        assert_eq!(
            output("print(starts_with(\"invoice.pdf\", \"inv\"));"),
            "true\n"
        );
        assert_eq!(
            output("print(ends_with(\"invoice.pdf\", \".csv\"));"),
            "false\n"
        );
        assert_eq!(
            runtime_error("x: Bool = contains(\"abc\", 1);"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Text".to_string(),
                found: "Int",
            }
        );
    }

    #[test]
    fn text_transformations() {
        assert_eq!(
            output("print(replace_all(\"a;b;c\", \";\", \",\"));"),
            "a,b,c\n"
        );
        assert_eq!(output("print(trim(\"  42 \\n\"));"), "42\n");
        assert_eq!(output("print(to_upper(\"été\"));"), "ÉTÉ\n");
        assert_eq!(output("print(to_lower(\"ÉTÉ\"));"), "été\n");
        assert_eq!(
            runtime_error("x: Text = replace_all(\"abc\", \"\", \"-\");"),
            RuntimeErrorKind::InvalidArgument {
                function: "replace_all",
                reason: "the text to replace cannot be empty".to_string(),
            }
        );
    }

    #[test]
    fn splitting_and_joining() {
        assert_eq!(
            output("print(split(\"a,,b\", \",\"));"),
            "[\"a\", \"\", \"b\"]\n"
        );
        assert_eq!(
            output("print(lines(\"one\\ntwo\\n\\nfour\\n\"));"),
            "[\"one\", \"two\", \"\", \"four\"]\n"
        );
        assert_eq!(output("print(join([\"a\", \"b\"], \", \"));"), "a, b\n");
        assert_eq!(
            output("empty: List<Text> = []; print(join(empty, \", \"));"),
            "\n"
        );
        assert!(matches!(
            runtime_error("x: List<Text> = split(\"abc\", \"\");"),
            RuntimeErrorKind::InvalidArgument {
                function: "split",
                ..
            }
        ));
        assert_eq!(
            runtime_error("x: Text = join([1, 2], \",\");"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Text".to_string(),
                found: "Int",
            }
        );
    }

    // Slarz has no `\r` escape: only a file can hold Windows line endings.
    #[test]
    fn lines_ignore_windows_line_endings() {
        let folder = fixture("windows-lines");
        std::fs::write(folder.join("data/windows.txt"), "one\r\ntwo\r\n").unwrap();
        let script = "permissions { read folder \"./data\"; }
            print(lines(check read_file(\"./data/windows.txt\")));";
        assert_eq!(run_in(&folder, script).unwrap(), "[\"one\", \"two\"]\n");
    }

    #[test]
    fn formatting_decimals() {
        assert_eq!(output("print(format_decimals(33.3333, 2));"), "33.33\n");
        // Halves round like `round`, away from zero (Rust's formatting alone
        // would give "2" and "0.12").
        assert_eq!(output("print(format_decimals(2.5, 0));"), "3\n");
        assert_eq!(output("print(format_decimals(-2.5, 0));"), "-3\n");
        assert_eq!(output("print(format_decimals(0.125, 2));"), "0.13\n");
        assert_eq!(output("print(format_decimals(2.675, 2));"), "2.68\n");
        // 1.005 * 100 is 100.49999999999999 in binary: the documented trap.
        assert_eq!(output("print(format_decimals(1.005, 2));"), "1.00\n");
        for invalid in ["-1", "18"] {
            assert!(matches!(
                runtime_error(&format!("x: Text = format_decimals(1.0, {invalid});")),
                RuntimeErrorKind::InvalidArgument {
                    function: "format_decimals",
                    ..
                }
            ));
        }
    }

    #[test]
    fn while_loop() {
        let body = "var count: Int = 0;
                    while count < 3 { count = count + 1; }
                    print(count);";
        assert_eq!(output(body), "3\n");
    }

    #[test]
    fn if_else_chain() {
        let body = "size: Int = 15;
                    if size < 10 { print(\"small\"); }
                    else if size < 20 { print(\"medium\"); }
                    else { print(\"large\"); }";
        assert_eq!(output(body), "medium\n");
    }

    #[test]
    fn logic_short_circuits() {
        assert_eq!(output("print(false and missing);"), "false\n");
        assert_eq!(output("print(true or missing);"), "true\n");
        assert_eq!(output("print(not (1 == 2));"), "true\n");
    }

    #[test]
    fn functions_and_recursion() {
        let body = "function factorial(n: Int) -> Int {
                        if n <= 1 { return 1; }
                        return n * factorial(n - 1);
                    }
                    print(factorial(5));";
        assert_eq!(output(body), "120\n");
    }

    #[test]
    fn functions_can_be_called_before_their_declaration() {
        let body = "greet(\"Wassim\");
                    function greet(name: Text) { print(\"Hello \" + name); }";
        assert_eq!(output(body), "Hello Wassim\n");
    }

    #[test]
    fn return_leaves_a_loop() {
        let body = "function first_over(limit: Int) -> Int {
                        var n: Int = 0;
                        while true {
                            n = n + 1;
                            if n > limit { return n; }
                        }
                        return 0;
                    }
                    print(first_over(4));";
        assert_eq!(output(body), "5\n");
    }

    #[test]
    fn lists() {
        let body = "names: List<Text> = [\"a\", \"b\"];
                    print(names);
                    print(length(names));
                    empty: List<Int> = [];
                    print(length(empty));
                    var total: Int = 0;
                    for n in [1, 2, 3] { total = total + n; }
                    print(total);
                    grid: List<List<Int>> = [[1], [2, 3]];
                    print(grid);";
        assert_eq!(output(body), "[\"a\", \"b\"]\n2\n0\n6\n[[1], [2, 3]]\n");
    }

    #[test]
    fn modifying_a_list_never_changes_another_variable() {
        let body = "a: List<Int> = [1, 2];
                    var b: List<Int> = a;
                    b = append(b, 3);
                    print(a);
                    print(b);";
        assert_eq!(output(body), "[1, 2]\n[1, 2, 3]\n");
    }

    #[test]
    fn appending_in_a_loop_stays_fast() {
        // Copying the list on every append would take billions of steps here.
        let body = "var items: List<Int> = [];
                    var n: Int = 0;
                    while n < 50000 {
                        items = append(items, n);
                        n = n + 1;
                    }
                    print(length(items));";
        let start = std::time::Instant::now();
        assert_eq!(output(body), "50000\n");
        assert!(start.elapsed().as_secs() < 5, "took {:?}", start.elapsed());
    }

    #[test]
    fn list_types() {
        assert_eq!(
            runtime_error("mixed: List<Int> = [1, \"two\"];"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Int".to_string(),
                found: "Text",
            }
        );
        assert_eq!(
            runtime_error("var n: List<Int> = []; n = append(n, \"x\");"),
            RuntimeErrorKind::TypeMismatch {
                expected: "List<Int>".to_string(),
                found: "List",
            }
        );
        assert_eq!(
            runtime_error("for x in 5 { print(x); }"),
            RuntimeErrorKind::TypeMismatch {
                expected: "List".to_string(),
                found: "Int",
            }
        );
    }

    #[test]
    fn list_folder_needs_read_permission() {
        let folder = fixture("list");
        let listed = run_in(
            &folder,
            "permissions { read folder \"./data\"; }
             for path in check list_folder(\"./data\") { print(path); }",
        );
        assert_eq!(listed, Ok("./data/2025/\n./data/notes.txt\n".into()));
        let outside = run_in(
            &folder,
            "permissions { read folder \"./data\"; }
             paths: List<Text> = check list_folder(\".\");",
        );
        assert_eq!(outside, Err(not_declared("read", ".")));
    }

    #[test]
    fn scopes() {
        assert_eq!(
            runtime_error("if true { bonus: Int = 5; } print(bonus);"),
            RuntimeErrorKind::UndefinedVariable("bonus".to_string())
        );
        assert_eq!(
            runtime_error("x: Int = 1; if true { x: Int = 2; }"),
            RuntimeErrorKind::AlreadyDeclared("x".to_string())
        );
        assert_eq!(
            runtime_error(
                "function peek() -> Int { return secret; }
                 function hide() { secret: Int = 1; print(peek()); }
                 hide();"
            ),
            RuntimeErrorKind::UndefinedVariable("secret".to_string())
        );
    }

    #[test]
    fn errors() {
        assert_eq!(
            runtime_error("x: Int = 1; x = 2;"),
            RuntimeErrorKind::AssignToConstant("x".to_string())
        );
        assert_eq!(
            runtime_error("x: Int = \"one\";"),
            RuntimeErrorKind::TypeMismatch {
                expected: "Int".to_string(),
                found: "Text",
            }
        );
        assert_eq!(
            runtime_error("x: Int = 1 + \"a\";"),
            RuntimeErrorKind::InvalidOperands {
                operator: "+",
                left: "Int",
                right: "Text",
            }
        );
        assert_eq!(
            runtime_error("x: Int = 7 / 2;"),
            RuntimeErrorKind::IntegerDivision
        );
        assert_eq!(
            runtime_error("x: Float = 1.0 / 0.0;"),
            RuntimeErrorKind::DivisionByZero
        );
        assert_eq!(
            runtime_error("x: Int = 9223372036854775807 + 1;"),
            RuntimeErrorKind::Overflow
        );
        assert_eq!(
            runtime_error("name: String = \"a\";").to_string(),
            "unknown type `String`: the text type is called `Text`"
        );
        assert_eq!(
            runtime_error("function f() -> Int { } x: Int = f();"),
            RuntimeErrorKind::MissingReturn("f".to_string())
        );
        assert_eq!(
            runtime_error("return;"),
            RuntimeErrorKind::ReturnOutsideFunction
        );
        assert_eq!(
            runtime_error("function loop() { loop(); } loop();"),
            RuntimeErrorKind::TooDeepRecursion
        );
    }
}
