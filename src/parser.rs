//! Builds the syntax tree from the tokens, with one function per grammar rule
//! (a "recursive descent" parser).

use std::fmt;

use crate::ast::{
    BinaryOperator, Block, Expression, ExpressionKind, Function, HttpMethod, Parameter, PathAccess,
    Permission, PermissionKind, Program, Statement, StatementKind, Target, Type, UnaryOperator,
};
use crate::token::{ForeignSymbol, Span, Token, TokenKind};

/// Why the tokens do not form a valid script.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseErrorKind {
    Expected {
        expected: &'static str,
        found: TokenKind,
    },
    Foreign(ForeignSymbol),
    ForeignKeyword(String),
    ChainedComparison,
    UnknownPermission(String),
    UnknownHttpMethod(String),
}

/// A parsing error and the part of the source it points at.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expected { expected, found } => write!(f, "expected {expected}, found {found}"),
            Self::Foreign(symbol) => write!(
                f,
                "`{}` is not Slarz: {}",
                symbol.spelling(),
                symbol.suggestion()
            ),
            Self::ForeignKeyword(word) if word == "let" || word == "const" => write!(
                f,
                "Slarz has no `{word}`: write `name: Type = value;` for a constant, \
                 or `var name: Type = value;` for a variable"
            ),
            Self::ForeignKeyword(word) => {
                write!(f, "functions are declared with `function`, not `{word}`")
            }
            Self::ChainedComparison => {
                write!(f, "comparisons cannot be chained: write `a < b and b < c`")
            }
            Self::UnknownPermission(word) => write!(
                f,
                "unknown permission `{word}`: use `read`, `write`, `network` or `env`"
            ),
            Self::UnknownHttpMethod(method) => write!(
                f,
                "unknown HTTP method `{method}`: use GET, POST, PUT, PATCH or DELETE"
            ),
        }
    }
}

type ParseResult<T> = Result<T, ParseError>;

/// Words from other languages that start a statement Slarz writes differently.
const FOREIGN_KEYWORDS: [&str; 5] = ["let", "const", "fn", "def", "func"];

const OR: [(TokenKind, BinaryOperator); 1] = [(TokenKind::Or, BinaryOperator::Or)];
const AND: [(TokenKind, BinaryOperator); 1] = [(TokenKind::And, BinaryOperator::And)];
const COMPARISONS: [(TokenKind, BinaryOperator); 6] = [
    (TokenKind::EqualEqual, BinaryOperator::Equal),
    (TokenKind::BangEqual, BinaryOperator::NotEqual),
    (TokenKind::Less, BinaryOperator::Less),
    (TokenKind::LessEqual, BinaryOperator::LessEqual),
    (TokenKind::Greater, BinaryOperator::Greater),
    (TokenKind::GreaterEqual, BinaryOperator::GreaterEqual),
];
const TERMS: [(TokenKind, BinaryOperator); 2] = [
    (TokenKind::Plus, BinaryOperator::Add),
    (TokenKind::Minus, BinaryOperator::Subtract),
];
const FACTORS: [(TokenKind, BinaryOperator); 2] = [
    (TokenKind::Star, BinaryOperator::Multiply),
    (TokenKind::Slash, BinaryOperator::Divide),
];

/// Parses a whole script, which must start with its `permissions` block.
pub fn parse(tokens: Vec<Token>) -> ParseResult<Program> {
    let mut parser = Parser::new(tokens);
    let permissions = parser.permissions()?;
    let mut statements = Vec::new();
    while !parser.at(&TokenKind::EndOfFile) {
        statements.push(parser.statement()?);
    }
    Ok(Program {
        permissions,
        statements,
    })
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(mut tokens: Vec<Token>) -> Self {
        if tokens
            .last()
            .is_none_or(|token| token.kind != TokenKind::EndOfFile)
        {
            let end = tokens.last().map_or(0, |token| token.span.end);
            tokens.push(Token {
                kind: TokenKind::EndOfFile,
                span: Span { start: end, end },
            });
        }
        Self {
            tokens,
            position: 0,
        }
    }

    // ----- Permissions header -----

    fn permissions(&mut self) -> ParseResult<Vec<Permission>> {
        self.expect(
            TokenKind::Permissions,
            "a `permissions { }` block at the start of the script",
        )?;
        self.expect(TokenKind::LeftBrace, "`{` after `permissions`")?;
        let mut permissions = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            permissions.push(self.permission()?);
        }
        Ok(permissions)
    }

    fn permission(&mut self) -> ParseResult<Permission> {
        let (word, start) =
            self.expect_identifier("a permission (`read`, `write`, `network` or `env`)")?;
        let kind = match word.as_str() {
            "read" => PermissionKind::Read(self.path_access()?),
            "write" => PermissionKind::Write(self.path_access()?),
            "network" => PermissionKind::Network {
                method: self.http_method()?,
                domain: self.expect_text("a domain in quotes, like \"api.github.com\"")?,
            },
            "env" => PermissionKind::Env {
                name: self.expect_text("a variable name in quotes, like \"GITHUB_TOKEN\"")?,
            },
            _ => {
                return Err(ParseError {
                    kind: ParseErrorKind::UnknownPermission(word),
                    span: start,
                });
            }
        };
        let end = self.expect(TokenKind::Semicolon, "`;` after the permission")?;
        Ok(Permission {
            kind,
            span: start.to(end),
        })
    }

    fn path_access(&mut self) -> ParseResult<PathAccess> {
        let target = match self.peek_identifier() {
            Some("folder") => Target::Folder,
            Some("file") => Target::File,
            _ => return Err(self.unexpected("`folder` or `file`")),
        };
        self.advance();
        let path = self.expect_text("a path in quotes, like \"./invoices\"")?;
        Ok(PathAccess { target, path })
    }

    fn http_method(&mut self) -> ParseResult<HttpMethod> {
        let (method, span) = self.expect_identifier("an HTTP method, like GET")?;
        match method.as_str() {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            "PUT" => Ok(HttpMethod::Put),
            "PATCH" => Ok(HttpMethod::Patch),
            "DELETE" => Ok(HttpMethod::Delete),
            _ => Err(ParseError {
                kind: ParseErrorKind::UnknownHttpMethod(method),
                span,
            }),
        }
    }

    // ----- Statements -----

    fn statement(&mut self) -> ParseResult<Statement> {
        let start = self.peek().span;
        if self.consume(&TokenKind::Var) {
            return self.declaration(start, true);
        }
        if self.consume(&TokenKind::If) {
            return self.if_statement(start);
        }
        if self.consume(&TokenKind::While) {
            return self.while_statement(start);
        }
        if self.consume(&TokenKind::For) {
            return self.for_statement(start);
        }
        if self.consume(&TokenKind::Function) {
            return self.function(start);
        }
        if self.consume(&TokenKind::Return) {
            return self.return_statement(start);
        }
        if let Some(name) = self.peek_identifier().map(str::to_string) {
            match self.peek_second().kind.clone() {
                TokenKind::Colon => return self.declaration(start, false),
                TokenKind::Equal => return self.assignment(start, name),
                TokenKind::Identifier(_) if FOREIGN_KEYWORDS.contains(&name.as_str()) => {
                    return Err(ParseError {
                        kind: ParseErrorKind::ForeignKeyword(name),
                        span: start,
                    });
                }
                // Two names in a row (`x Int = 5;`): most likely a declaration
                // missing its `:`, so let that rule report what it expected.
                TokenKind::Identifier(_) => return self.declaration(start, false),
                _ => {}
            }
        }
        self.expression_statement(start)
    }

    fn declaration(&mut self, start: Span, mutable: bool) -> ParseResult<Statement> {
        let (name, _) = self.expect_identifier("a variable name")?;
        self.expect(TokenKind::Colon, "`:` and a type after the variable name")?;
        let declared_type = self.parse_type()?;
        self.expect(
            TokenKind::Equal,
            "`=` and a value: every variable gets a value when it is declared",
        )?;
        let value = self.expression()?;
        let end = self.expect(TokenKind::Semicolon, "`;` at the end of the declaration")?;
        Ok(Statement {
            kind: StatementKind::Declaration {
                name,
                mutable,
                declared_type,
                value,
            },
            span: start.to(end),
        })
    }

    fn assignment(&mut self, start: Span, name: String) -> ParseResult<Statement> {
        self.advance();
        self.expect(TokenKind::Equal, "`=`")?;
        let value = self.expression()?;
        let end = self.expect(TokenKind::Semicolon, "`;` at the end of the assignment")?;
        Ok(Statement {
            kind: StatementKind::Assignment { name, value },
            span: start.to(end),
        })
    }

    fn expression_statement(&mut self, start: Span) -> ParseResult<Statement> {
        let expression = self.expression()?;
        let end = self.expect(TokenKind::Semicolon, "`;` at the end of the statement")?;
        Ok(Statement {
            kind: StatementKind::Expression(expression),
            span: start.to(end),
        })
    }

    fn if_statement(&mut self, start: Span) -> ParseResult<Statement> {
        let condition = self.expression()?;
        let then_block = self.block()?;
        let else_block = if self.consume(&TokenKind::Else) {
            let else_start = self.peek().span;
            if self.consume(&TokenKind::If) {
                Some(vec![self.if_statement(else_start)?])
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(self.finish(
            StatementKind::If {
                condition,
                then_block,
                else_block,
            },
            start,
        ))
    }

    fn while_statement(&mut self, start: Span) -> ParseResult<Statement> {
        let condition = self.expression()?;
        let body = self.block()?;
        Ok(self.finish(StatementKind::While { condition, body }, start))
    }

    fn for_statement(&mut self, start: Span) -> ParseResult<Statement> {
        let (variable, _) = self.expect_identifier("a loop variable name")?;
        self.expect(TokenKind::In, "`in` after the loop variable")?;
        let iterable = self.expression()?;
        let body = self.block()?;
        Ok(self.finish(
            StatementKind::For {
                variable,
                iterable,
                body,
            },
            start,
        ))
    }

    fn function(&mut self, start: Span) -> ParseResult<Statement> {
        let (name, _) = self.expect_identifier("a function name")?;
        self.expect(TokenKind::LeftParen, "`(` after the function name")?;
        let parameters = self.list_until(
            &TokenKind::RightParen,
            Self::parameter,
            "`,` or `)` after the parameter",
        )?;
        let return_type = if self.consume(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.block()?;
        let function = Function {
            name,
            parameters,
            return_type,
            body,
        };
        Ok(self.finish(StatementKind::Function(function), start))
    }

    fn parameter(&mut self) -> ParseResult<Parameter> {
        let (name, start) = self.expect_identifier("a parameter name")?;
        self.expect(TokenKind::Colon, "`:` and a type after the parameter name")?;
        let declared_type = self.parse_type()?;
        Ok(Parameter {
            name,
            span: start.to(declared_type.span),
            declared_type,
        })
    }

    fn return_statement(&mut self, start: Span) -> ParseResult<Statement> {
        let value = if self.at(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.expression()?)
        };
        let end = self.expect(TokenKind::Semicolon, "`;` after the returned value")?;
        Ok(Statement {
            kind: StatementKind::Return(value),
            span: start.to(end),
        })
    }

    fn block(&mut self) -> ParseResult<Block> {
        self.expect(TokenKind::LeftBrace, "`{` to start a block")?;
        let mut statements = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            if self.at(&TokenKind::EndOfFile) {
                return Err(self.unexpected("`}` to close the block"));
            }
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        let (name, start) = self.expect_identifier("a type, like `Int` or `Text`")?;
        let mut arguments = Vec::new();
        if self.consume(&TokenKind::Less) {
            loop {
                arguments.push(self.parse_type()?);
                if self.consume(&TokenKind::Greater) {
                    break;
                }
                self.expect(TokenKind::Comma, "`,` or `>` in the type")?;
            }
        }
        Ok(Type {
            name,
            arguments,
            span: start.to(self.previous_span()),
        })
    }

    // ----- Expressions, from the loosest operator to the tightest -----

    fn expression(&mut self) -> ParseResult<Expression> {
        self.or()
    }

    fn or(&mut self) -> ParseResult<Expression> {
        self.left_associative(Self::and, &OR)
    }

    fn and(&mut self) -> ParseResult<Expression> {
        self.left_associative(Self::not, &AND)
    }

    // `not` binds looser than comparisons, as in Python: `not a == b`
    // means `not (a == b)`.
    fn not(&mut self) -> ParseResult<Expression> {
        let start = self.peek().span;
        if self.consume(&TokenKind::Not) {
            let operand = self.not()?;
            return Ok(unary(UnaryOperator::Not, operand, start));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> ParseResult<Expression> {
        let left = self.term()?;
        let Some(operator) = self.consume_operator(&COMPARISONS) else {
            return Ok(left);
        };
        let right = self.term()?;
        // `a < b < c` means different things in different languages: refuse it.
        if COMPARISONS.iter().any(|(kind, _)| self.at(kind)) {
            return Err(ParseError {
                kind: ParseErrorKind::ChainedComparison,
                span: self.peek().span,
            });
        }
        Ok(binary(left, operator, right))
    }

    fn term(&mut self) -> ParseResult<Expression> {
        self.left_associative(Self::factor, &TERMS)
    }

    fn factor(&mut self) -> ParseResult<Expression> {
        self.left_associative(Self::unary, &FACTORS)
    }

    fn unary(&mut self) -> ParseResult<Expression> {
        let start = self.peek().span;
        if self.consume(&TokenKind::Minus) {
            let operand = self.unary()?;
            return Ok(unary(UnaryOperator::Negate, operand, start));
        }
        if self.consume(&TokenKind::Check) {
            let operand = self.unary()?;
            return Ok(Expression {
                span: start.to(operand.span),
                kind: ExpressionKind::Check(Box::new(operand)),
            });
        }
        self.primary()
    }

    fn primary(&mut self) -> ParseResult<Expression> {
        let token = self.peek().clone();
        let kind = match token.kind {
            TokenKind::Integer(value) => ExpressionKind::Integer(value),
            TokenKind::Float(value) => ExpressionKind::Float(value),
            TokenKind::Text(text) => ExpressionKind::Text(text),
            TokenKind::True => ExpressionKind::Bool(true),
            TokenKind::False => ExpressionKind::Bool(false),
            TokenKind::Identifier(name) => return self.variable_or_call(name, token.span),
            TokenKind::LeftParen => return self.parenthesized(),
            TokenKind::LeftBracket => return self.list_literal(token.span),
            _ => return Err(self.unexpected("an expression")),
        };
        self.advance();
        Ok(Expression {
            kind,
            span: token.span,
        })
    }

    fn variable_or_call(&mut self, name: String, start: Span) -> ParseResult<Expression> {
        self.advance();
        if !self.consume(&TokenKind::LeftParen) {
            return Ok(Expression {
                kind: ExpressionKind::Variable(name),
                span: start,
            });
        }
        let arguments = self.list_until(
            &TokenKind::RightParen,
            Self::expression,
            "`,` or `)` after the argument",
        )?;
        Ok(Expression {
            kind: ExpressionKind::Call {
                function: name,
                arguments,
            },
            span: start.to(self.previous_span()),
        })
    }

    fn list_literal(&mut self, start: Span) -> ParseResult<Expression> {
        self.advance();
        let items = self.list_until(
            &TokenKind::RightBracket,
            Self::expression,
            "`,` or `]` in the list",
        )?;
        Ok(Expression {
            kind: ExpressionKind::List(items),
            span: start.to(self.previous_span()),
        })
    }

    fn parenthesized(&mut self) -> ParseResult<Expression> {
        self.advance();
        let inner = self.expression()?;
        self.expect(TokenKind::RightParen, "`)` to close the parenthesis")?;
        Ok(inner)
    }

    fn left_associative(
        &mut self,
        operand: fn(&mut Self) -> ParseResult<Expression>,
        operators: &[(TokenKind, BinaryOperator)],
    ) -> ParseResult<Expression> {
        let mut left = operand(self)?;
        while let Some(operator) = self.consume_operator(operators) {
            let right = operand(self)?;
            left = binary(left, operator, right);
        }
        Ok(left)
    }

    // ----- Token helpers -----

    /// Comma-separated items up to `closing`, which the opening token implies.
    fn list_until<T>(
        &mut self,
        closing: &TokenKind,
        item: fn(&mut Self) -> ParseResult<T>,
        separator: &'static str,
    ) -> ParseResult<Vec<T>> {
        let mut items = Vec::new();
        if self.consume(closing) {
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            if self.consume(closing) {
                return Ok(items);
            }
            self.expect(TokenKind::Comma, separator)?;
        }
    }

    fn consume_operator(
        &mut self,
        operators: &[(TokenKind, BinaryOperator)],
    ) -> Option<BinaryOperator> {
        let operator = operators
            .iter()
            .find(|(kind, _)| self.at(kind))
            .map(|(_, operator)| *operator)?;
        self.advance();
        Some(operator)
    }

    // The token list always ends with `EndOfFile` (see `new`), and the
    // position never goes past it, so these indexes are always valid.
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn peek_second(&self) -> &Token {
        &self.tokens[(self.position + 1).min(self.tokens.len() - 1)]
    }

    fn previous_span(&self) -> Span {
        self.tokens[self.position.saturating_sub(1)].span
    }

    fn peek_identifier(&self) -> Option<&str> {
        match &self.peek().kind {
            TokenKind::Identifier(name) => Some(name),
            _ => None,
        }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    fn advance(&mut self) -> Span {
        let span = self.peek().span;
        if self.position < self.tokens.len() - 1 {
            self.position += 1;
        }
        span
    }

    fn consume(&mut self, kind: &TokenKind) -> bool {
        let matches = self.at(kind);
        if matches {
            self.advance();
        }
        matches
    }

    fn expect(&mut self, kind: TokenKind, expected: &'static str) -> ParseResult<Span> {
        if self.at(&kind) {
            Ok(self.advance())
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn expect_identifier(&mut self, expected: &'static str) -> ParseResult<(String, Span)> {
        let Some(name) = self.peek_identifier().map(str::to_string) else {
            return Err(self.unexpected(expected));
        };
        Ok((name, self.advance()))
    }

    fn expect_text(&mut self, expected: &'static str) -> ParseResult<String> {
        let TokenKind::Text(text) = &self.peek().kind else {
            return Err(self.unexpected(expected));
        };
        let text = text.clone();
        self.advance();
        Ok(text)
    }

    fn unexpected(&self, expected: &'static str) -> ParseError {
        let token = self.peek();
        let kind = match &token.kind {
            TokenKind::Foreign(symbol) => ParseErrorKind::Foreign(*symbol),
            found => ParseErrorKind::Expected {
                expected,
                found: found.clone(),
            },
        };
        ParseError {
            kind,
            span: token.span,
        }
    }

    fn finish(&self, kind: StatementKind, start: Span) -> Statement {
        Statement {
            kind,
            span: start.to(self.previous_span()),
        }
    }
}

fn binary(left: Expression, operator: BinaryOperator, right: Expression) -> Expression {
    Expression {
        span: left.span.to(right.span),
        kind: ExpressionKind::Binary {
            left: Box::new(left),
            operator,
            right: Box::new(right),
        },
    }
}

fn unary(operator: UnaryOperator, operand: Expression, start: Span) -> Expression {
    Expression {
        span: start.to(operand.span),
        kind: ExpressionKind::Unary {
            operator,
            operand: Box::new(operand),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn program(source: &str) -> Program {
        parse(tokenize(source).unwrap()).unwrap()
    }

    fn statements(body: &str) -> Vec<StatementKind> {
        program(&format!("permissions {{ }}\n{body}"))
            .statements
            .into_iter()
            .map(|statement| statement.kind)
            .collect()
    }

    fn error(source: &str) -> ParseErrorKind {
        parse(tokenize(source).unwrap()).unwrap_err().kind
    }

    fn body_error(body: &str) -> ParseErrorKind {
        error(&format!("permissions {{ }}\n{body}"))
    }

    /// Parses `source` as the value of a declaration and prints its tree
    /// in prefix form, so that precedence is visible: `(Add 1 (Multiply 2 3))`.
    fn tree(source: &str) -> String {
        match statements(&format!("x: Int = {source};")).remove(0) {
            StatementKind::Declaration { value, .. } => show(&value),
            other => panic!("expected a declaration, got {other:?}"),
        }
    }

    fn show(expression: &Expression) -> String {
        match &expression.kind {
            ExpressionKind::Integer(value) => value.to_string(),
            ExpressionKind::Float(value) => value.to_string(),
            ExpressionKind::Text(text) => format!("{text:?}"),
            ExpressionKind::Bool(value) => value.to_string(),
            ExpressionKind::Variable(name) => name.clone(),
            ExpressionKind::List(items) => {
                let items: Vec<String> = items.iter().map(show).collect();
                format!("[{}]", items.join(", "))
            }
            ExpressionKind::Unary { operator, operand } => {
                format!("({operator:?} {})", show(operand))
            }
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => format!("({operator:?} {} {})", show(left), show(right)),
            ExpressionKind::Call {
                function,
                arguments,
            } => {
                let arguments: String = arguments.iter().map(|a| format!(" {}", show(a))).collect();
                format!("(call {function}{arguments})")
            }
            ExpressionKind::Check(inner) => format!("(check {})", show(inner)),
        }
    }

    #[test]
    fn permissions_header() {
        let program = program(
            r#"permissions {
                read folder "./invoices";
                write file "./report.txt";
                network GET "api.github.com";
                env "GITHUB_TOKEN";
            }"#,
        );
        let kinds: Vec<PermissionKind> = program
            .permissions
            .into_iter()
            .map(|permission| permission.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                PermissionKind::Read(PathAccess {
                    target: Target::Folder,
                    path: "./invoices".to_string(),
                }),
                PermissionKind::Write(PathAccess {
                    target: Target::File,
                    path: "./report.txt".to_string(),
                }),
                PermissionKind::Network {
                    method: HttpMethod::Get,
                    domain: "api.github.com".to_string(),
                },
                PermissionKind::Env {
                    name: "GITHUB_TOKEN".to_string(),
                },
            ]
        );
    }

    #[test]
    fn declarations_and_assignment() {
        let parsed = statements("total: Int = 0; var names: List<Text> = empty(); total = 1;");
        let StatementKind::Declaration {
            name,
            mutable,
            declared_type,
            ..
        } = &parsed[1]
        else {
            panic!("expected a declaration");
        };
        assert_eq!(name, "names");
        assert!(mutable);
        assert_eq!(declared_type.name, "List");
        assert_eq!(declared_type.arguments[0].name, "Text");
        assert!(matches!(
            &parsed[0],
            StatementKind::Declaration { mutable: false, .. }
        ));
        assert!(matches!(&parsed[2], StatementKind::Assignment { name, .. } if name == "total"));
    }

    #[test]
    fn operator_precedence() {
        assert_eq!(tree("1 + 2 * 3"), "(Add 1 (Multiply 2 3))");
        assert_eq!(tree("(1 + 2) * 3"), "(Multiply (Add 1 2) 3)");
        assert_eq!(tree("1 - 2 - 3"), "(Subtract (Subtract 1 2) 3)");
        assert_eq!(tree("-a * b"), "(Multiply (Negate a) b)");
        assert_eq!(
            tree("not a == b and c or d"),
            "(Or (And (Not (Equal a b)) c) d)"
        );
    }

    #[test]
    fn calls_and_check() {
        assert_eq!(
            tree("check read_file(\"a.csv\", 2)"),
            "(check (call read_file \"a.csv\" 2))"
        );
        assert_eq!(tree("now()"), "(call now)");
        assert_eq!(tree("[1, 2 + 3, []]"), "[1, (Add 2 3), []]");
    }

    #[test]
    fn control_flow() {
        let parsed = statements(
            "if a { print(1); } else if b { print(2); } else { print(3); }
             while running { tick(); }
             for file in files { print(file); }",
        );
        let StatementKind::If { else_block, .. } = &parsed[0] else {
            panic!("expected an if");
        };
        let nested = &else_block.as_ref().unwrap()[0].kind;
        assert!(matches!(
            nested,
            StatementKind::If {
                else_block: Some(_),
                ..
            }
        ));
        assert!(matches!(&parsed[1], StatementKind::While { .. }));
        assert!(matches!(&parsed[2], StatementKind::For { variable, .. } if variable == "file"));
    }

    #[test]
    fn functions() {
        let parsed = statements(
            "function add(a: Int, b: Int) -> Int { return a + b; }
             function log(message: Text) { print(message); return; }",
        );
        let StatementKind::Function(add) = &parsed[0] else {
            panic!("expected a function");
        };
        assert_eq!(add.parameters.len(), 2);
        assert_eq!(add.return_type.as_ref().unwrap().name, "Int");
        assert!(matches!(&parsed[1], StatementKind::Function(log) if log.return_type.is_none()));
    }

    #[test]
    fn errors() {
        assert!(matches!(
            error("x: Int = 1;"),
            ParseErrorKind::Expected {
                found: TokenKind::Identifier(_),
                ..
            }
        ));
        assert_eq!(
            body_error("x: = 5;").to_string(),
            "expected a type, like `Int` or `Text`, found `=`"
        );
        assert_eq!(
            body_error("x: Int = 5"),
            ParseErrorKind::Expected {
                expected: "`;` at the end of the declaration",
                found: TokenKind::EndOfFile,
            }
        );
        assert_eq!(
            body_error("ok: Bool = a && b;"),
            ParseErrorKind::Foreign(ForeignSymbol::AndAnd)
        );
        assert_eq!(
            body_error("let x = 5;"),
            ParseErrorKind::ForeignKeyword("let".to_string())
        );
        assert_eq!(
            body_error("x Int = 5;").to_string(),
            "expected `:` and a type after the variable name, found the name `Int`"
        );
        assert_eq!(
            body_error("ok: Bool = 1 < 2 < 3;"),
            ParseErrorKind::ChainedComparison
        );
        assert_eq!(
            error("permissions { delete file \"x\"; }"),
            ParseErrorKind::UnknownPermission("delete".to_string())
        );
        assert_eq!(
            error("permissions { network FETCH \"x.com\"; }"),
            ParseErrorKind::UnknownHttpMethod("FETCH".to_string())
        );
    }
}
