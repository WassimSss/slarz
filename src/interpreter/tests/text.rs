//! Text functions.

use super::*;

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
