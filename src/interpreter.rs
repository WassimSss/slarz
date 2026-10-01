//! Runs a program by walking its syntax tree.

mod environment;
mod error;
mod expressions;
mod functions;
mod operators;
mod statements;
mod types;

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use crate::ast::{Function, Program};
use crate::permissions::Permissions;
use crate::value::Value;

use environment::{Scope, Variable};
use error::{NO_VALUE, not_optional_or_result};
pub(crate) use error::{RunResult, error, type_mismatch};
pub use error::{RuntimeError, RuntimeErrorKind};

/// Deep enough for real scripts, shallow enough to stop runaway recursion
/// before the interpreter itself runs out of stack.
const MAX_CALL_DEPTH: usize = 100;

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
