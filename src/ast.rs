//! The syntax tree: what the parser builds from the tokens.

use std::fmt;

use crate::token::Span;

/// A whole script: its permissions header, then its statements.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub permissions: Vec<Permission>,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    pub kind: PermissionKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionKind {
    Read(PathAccess),
    Write(PathAccess),
    Network { method: HttpMethod, domain: String },
    Env { name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathAccess {
    pub target: Target,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Folder,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

pub type Block = Vec<Statement>;

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Declaration {
        name: String,
        mutable: bool,
        declared_type: Type,
        value: Expression,
    },
    Assignment {
        name: String,
        value: Expression,
    },
    Expression(Expression),
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expression,
        body: Block,
    },
    For {
        variable: String,
        iterable: Expression,
        body: Block,
    },
    Function(Function),
    Return(Option<Expression>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub declared_type: Type,
    pub span: Span,
}

/// A written type, such as `Int` or `Result<Text, Error>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub name: String,
    pub arguments: Vec<Type>,
    pub span: Span,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some((first, rest)) = self.arguments.split_first() {
            write!(f, "<{first}")?;
            for argument in rest {
                write!(f, ", {argument}")?;
            }
            write!(f, ">")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}

// Sub-expressions are boxed: an expression that contains itself directly
// would have an infinite size.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    Integer(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    List(Vec<Expression>),
    Variable(String),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
    Call {
        function: String,
        arguments: Vec<Expression>,
    },
    Check(Box<Expression>),
    /// `value otherwise fallback`: the fallback is evaluated only on failure.
    Otherwise {
        value: Box<Expression>,
        fallback: Box<Expression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}
