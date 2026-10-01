//! Runs every script in `tests/scripts` with the real `slarz` binary and
//! compares what happens with the files next to it:
//!
//! - `name.out`: what the script must print (nothing if the file is absent);
//! - `name.err`: the error it must report (none if the file is absent);
//! - `name.code`: its exit code, when it is neither 0 (success) nor 1 (error).
//!
//! Every script runs with the environment variable `SLARZ_GOLDEN_SECRET` set
//! to `s3cr3t`, so that `env` can be tested.
//!
//! Adding a test means adding a script and its expected files: no Rust needed.

// The whole file is test code: stopping on the spot is the point.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn scripts_behave_as_expected() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts");
    let mut scripts: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "slz"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no scripts in {}", folder.display());

    let failures: Vec<String> = scripts
        .iter()
        .filter_map(|script| check_script(&folder, script))
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} scripts failed:\n\n{}",
        failures.len(),
        scripts.len(),
        failures.join("\n\n")
    );
}

fn check_script(folder: &Path, script: &Path) -> Option<String> {
    let name = script.file_name()?.to_string_lossy().into_owned();
    // Launched from the script's folder, so messages mention `name.slz`
    // rather than a path that depends on the machine.
    let result = Command::new(env!("CARGO_BIN_EXE_slarz"))
        .arg(&name)
        .current_dir(folder)
        .env("SLARZ_GOLDEN_SECRET", "s3cr3t")
        .output()
        .unwrap();

    let expected = |extension: &str| {
        fs::read_to_string(script.with_extension(extension))
            .map(|text| normalize(&text))
            .unwrap_or_default()
    };
    let expected_code = match fs::read_to_string(script.with_extension("code")) {
        Ok(code) => code.trim().parse().unwrap(),
        Err(_) if script.with_extension("err").exists() => 1,
        Err(_) => 0,
    };

    let output = normalize(&String::from_utf8_lossy(&result.stdout));
    let errors = normalize(&String::from_utf8_lossy(&result.stderr));
    let code = result.status.code().unwrap_or(-1);

    let mut problems = Vec::new();
    if output != expected("out") {
        problems.push(format!(
            "  output\n    expected: {:?}\n    got:      {output:?}",
            expected("out")
        ));
    }
    if errors != expected("err") {
        problems.push(format!(
            "  errors\n    expected: {:?}\n    got:      {errors:?}",
            expected("err")
        ));
    }
    if code != expected_code {
        problems.push(format!(
            "  exit code\n    expected: {expected_code}\n    got:      {code}"
        ));
    }
    (!problems.is_empty()).then(|| format!("{name}\n{}", problems.join("\n")))
}

// Git may turn line endings into `\r\n` on Windows.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The scripts shown to visitors must keep working: `examples/`, and every
/// Slarz code block of the README (those starting with `permissions`).
#[test]
fn examples_and_readme_run() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut failures = Vec::new();

    for entry in fs::read_dir(root.join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "slz") {
            failures.extend(run_successfully(&path));
        }
    }

    let readme = normalize(&fs::read_to_string(root.join("README.md")).unwrap());
    let folder = std::env::temp_dir().join(format!("slarz-readme-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    for (index, block) in code_blocks(&readme).iter().enumerate() {
        if block.trim_start().starts_with("permissions") {
            let script = folder.join(format!("readme-{index}.slz"));
            fs::write(&script, block).unwrap();
            failures.extend(run_successfully(&script));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn run_successfully(script: &Path) -> Option<String> {
    let result = Command::new(env!("CARGO_BIN_EXE_slarz"))
        .arg(script.file_name()?)
        .current_dir(script.parent()?)
        .output()
        .unwrap();
    (!result.status.success()).then(|| {
        format!(
            "{} failed:\n{}",
            script.display(),
            String::from_utf8_lossy(&result.stderr)
        )
    })
}

fn code_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match (&mut current, line.starts_with("```")) {
            (None, true) => current = Some(String::new()),
            (Some(_), true) => blocks.extend(current.take()),
            (Some(block), false) => {
                block.push_str(line);
                block.push('\n');
            }
            (None, false) => {}
        }
    }
    blocks
}
