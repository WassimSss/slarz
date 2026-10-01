//! Arithmetic, conversions, rounding and integer division.

use super::*;

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
