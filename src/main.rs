use std::fmt::Display;
use std::path::Path;
use std::process::ExitCode;

use slarz::token::Span;

/// Exit code for a permission violation, distinct from ordinary errors (1)
/// so that a scheduler or a CI job can tell an attempted breach apart.
const PERMISSION_VIOLATION: u8 = 3;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("error: no script to run\nusage: slarz <script.slz>");
        return ExitCode::FAILURE;
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let tokens = match slarz::lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => return report(&path, &source, error.span, error.kind),
    };

    let program = match slarz::parser::parse(tokens) {
        Ok(program) => program,
        Err(error) => return report(&path, &source, error.span, error.kind),
    };

    let script_folder = Path::new(&path)
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match slarz::interpreter::run(&program, script_folder, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind.is_permission_violation() => {
            report(&path, &source, error.span, error.kind);
            ExitCode::from(PERMISSION_VIOLATION)
        }
        Err(error) => report(&path, &source, error.span, error.kind),
    }
}

fn report(path: &str, source: &str, span: Span, message: impl Display) -> ExitCode {
    let (line, column) = span.line_column(source);
    eprintln!("error: {path}:{line}:{column}: {message}");
    ExitCode::FAILURE
}
