//! Errors that stop the script, and their messages.

use super::*;

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
