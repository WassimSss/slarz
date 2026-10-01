//! `Result` and `Optional`: `check`, `otherwise`, `get` and `if name: Type = ...`.

use super::*;

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
