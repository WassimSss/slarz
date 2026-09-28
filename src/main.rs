use std::process::ExitCode;

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

    match slarz::lexer::tokenize(&source) {
        Ok(tokens) => {
            for token in tokens {
                println!("{:?}", token.kind);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let (line, column) = error.span.line_column(&source);
            eprintln!("error: {path}:{line}:{column}: {}", error.kind);
            ExitCode::FAILURE
        }
    }
}
