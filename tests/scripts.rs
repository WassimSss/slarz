//! Runs every script in `tests/scripts` with the real `slarz` binary and
//! compares what happens with the files next to it:
//!
//! - `name.out`: what the script must print (nothing if the file is absent);
//! - `name.err`: the error it must report (none if the file is absent);
//! - `name.code`: its exit code, when it is neither 0 (success) nor 1 (error).
//!
//! Adding a test means adding a script and its expected files: no Rust needed.

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
