//! Tests of the interpreter, one file per theme. Most run a whole script and
//! look at what it prints or at the error that stops it.

mod control_flow;
mod errors;
mod failures;
mod json;
mod lists;
mod numbers;
mod permissions;
mod text;

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
