//! Lists: building, copying, appending, typing.

use super::*;

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
