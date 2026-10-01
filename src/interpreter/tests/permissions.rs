//! Reading and writing files, environment variables: only what the header declares.

use super::*;

fn read_in_data(folder: &Path, path: &str) -> Result<String, RuntimeErrorKind> {
    let source =
        format!("permissions {{ read folder \"./data\"; }}\nprint(check read_file(\"{path}\"));");
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
