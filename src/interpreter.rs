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
use crate::permissions::Permissions;
use crate::token::Span;
use crate::value::Value;

/// Deep enough for real scripts, shallow enough to stop runaway recursion
/// before the interpreter itself runs out of stack.
const MAX_CALL_DEPTH: usize = 100;

const BUILTINS: [&str; 2] = ["print", "read_file"];

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
    MissingPermissionPath(String),
    /// An operation failed and `check` passed the failure up.
    Failed(String),
}

impl RuntimeErrorKind {
    pub fn is_permission_violation(&self) -> bool {
        matches!(self, Self::PermissionDenied { .. })
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
                "unknown type `{name}`: the available types are `Int`, `Float`, `Text`, `Bool` \
                 and `Result<T, Error>`"
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
                 means for integers: write `7.0 / 2.0`"
            ),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::Overflow => write!(f, "integer overflow: the result does not fit in an `Int`"),
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
            Self::MissingPermissionPath(path) => write!(
                f,
                "this permission points to `{path}`, which does not exist"
            ),
            Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

type RunResult<T> = Result<T, RuntimeError>;

/// Runs `program`, writing what it prints to `output`. Paths in the script
/// are relative to `script_folder`, the folder that holds the script.
pub fn run(program: &Program, script_folder: &Path, output: &mut impl Write) -> RunResult<()> {
    let permissions = Permissions::new(&program.permissions, script_folder).map_err(|missing| {
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
                if BUILTINS.contains(&function.name.as_str())
                    || self.functions.contains_key(&function.name)
                {
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
                self.declare(name, value, *mutable, span)?;
            }
            StatementKind::Assignment { name, value } => {
                let value = self.evaluate(value)?;
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
            StatementKind::While { condition, body } => {
                while self.condition(condition)? {
                    if let Flow::Return(value) = self.execute_block(body)? {
                        return Ok(Flow::Return(value));
                    }
                }
            }
            StatementKind::For { .. } => {
                return Err(error(
                    RuntimeErrorKind::NotSupportedYet(
                        "`for` loops are not supported yet: they need lists",
                    ),
                    span,
                ));
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

    fn declare(&mut self, name: &str, value: Value, mutable: bool, span: Span) -> RunResult<()> {
        if self.lookup(name).is_some() {
            return Err(error(
                RuntimeErrorKind::AlreadyDeclared(name.to_string()),
                span,
            ));
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), Variable { value, mutable });
        }
        Ok(())
    }

    fn assign(&mut self, name: &str, value: Value, span: Span) -> RunResult<()> {
        let variable = self
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
            .ok_or_else(|| error(RuntimeErrorKind::UndefinedVariable(name.to_string()), span))?;
        if !variable.mutable {
            return Err(error(
                RuntimeErrorKind::AssignToConstant(name.to_string()),
                span,
            ));
        }
        if variable.value.type_name() != value.type_name() {
            return Err(error(
                RuntimeErrorKind::TypeMismatch {
                    expected: variable.value.type_name().to_string(),
                    found: value.type_name(),
                },
                span,
            ));
        }
        variable.value = value;
        Ok(())
    }

    // ----- Expressions -----

    fn evaluate(&mut self, expression: &Expression) -> RunResult<Value> {
        let span = expression.span;
        match &expression.kind {
            ExpressionKind::Integer(value) => Ok(Value::Integer(*value)),
            ExpressionKind::Float(value) => Ok(Value::Float(*value)),
            ExpressionKind::Text(text) => Ok(Value::Text(text.clone())),
            ExpressionKind::Bool(value) => Ok(Value::Bool(*value)),
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
                Value::Success(value) => Ok(*value),
                Value::Failure(message) => Err(error(RuntimeErrorKind::Failed(message), span)),
                other => Err(error(
                    RuntimeErrorKind::TypeMismatch {
                        expected: "Result".to_string(),
                        found: other.type_name(),
                    },
                    inner.span,
                )),
            },
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression], span: Span) -> RunResult<Value> {
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            values.push(self.evaluate(argument)?);
        }
        match name {
            "print" => return self.print(values, span),
            "read_file" => return self.read_file(values, span),
            _ => {}
        }
        let function =
            self.functions.get(name).cloned().ok_or_else(|| {
                error(RuntimeErrorKind::UndefinedFunction(name.to_string()), span)
            })?;
        self.call_function(&function, values, span)
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

    // ----- Built-in functions -----

    fn print(&mut self, arguments: Vec<Value>, span: Span) -> RunResult<Value> {
        let value = single_argument("print", arguments, span)?;
        writeln!(self.output, "{value}")
            .map_err(|_| error(RuntimeErrorKind::OutputFailed, span))?;
        Ok(Value::Nothing)
    }

    // The permission is checked before the disk is touched: a denied path is
    // never even looked at.
    fn read_file(&mut self, arguments: Vec<Value>, span: Span) -> RunResult<Value> {
        let path = match single_argument("read_file", arguments, span)? {
            Value::Text(path) => path,
            other => {
                return Err(error(
                    RuntimeErrorKind::TypeMismatch {
                        expected: "Text".to_string(),
                        found: other.type_name(),
                    },
                    span,
                ));
            }
        };
        let Some(real_path) = self.permissions.resolve_read(&path) else {
            return Err(error(
                RuntimeErrorKind::PermissionDenied {
                    access: "read",
                    path,
                },
                span,
            ));
        };
        Ok(match std::fs::read_to_string(real_path) {
            Ok(content) => Value::Success(Box::new(Value::Text(content))),
            Err(reason) => Value::Failure(format!("cannot read `{path}`: {reason}")),
        })
    }
}

fn single_argument(function: &str, arguments: Vec<Value>, span: Span) -> RunResult<Value> {
    let [value] = <[Value; 1]>::try_from(arguments).map_err(|arguments| {
        error(
            RuntimeErrorKind::WrongArgumentCount {
                function: function.to_string(),
                expected: 1,
                found: arguments.len(),
            },
            span,
        )
    })?;
    Ok(value)
}

fn error(kind: RuntimeErrorKind, span: Span) -> RuntimeError {
    RuntimeError { kind, span }
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
        ("Int" | "Float" | "Text" | "Bool", []) => Ok(value.type_name() == declared.name),
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
        run(&program(body), Path::new("."), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn runtime_error(body: &str) -> RuntimeErrorKind {
        run(&program(body), Path::new("."), &mut Vec::new())
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
        run(&program, folder, &mut output).map_err(|error| error.kind)?;
        Ok(String::from_utf8(output).unwrap())
    }

    fn read_in_data(folder: &Path, path: &str) -> Result<String, RuntimeErrorKind> {
        let source = format!(
            "permissions {{ read folder \"./data\"; }}\nprint(check read_file(\"{path}\"));"
        );
        run_in(folder, &source)
    }

    fn denied(path: &str) -> RuntimeErrorKind {
        RuntimeErrorKind::PermissionDenied {
            access: "read",
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
            assert_eq!(read_in_data(&folder, path), Err(denied(path)), "{path}");
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
            Err(denied("./database.txt"))
        );
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
                expected: "Result".to_string(),
                found: "Int",
            }
        );
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
