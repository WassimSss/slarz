//! `if`, loops, functions and scopes.

use super::*;

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
