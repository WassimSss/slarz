//! Reading JSON documents.

use super::*;

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
