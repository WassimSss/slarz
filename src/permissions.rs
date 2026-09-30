//! Decides what a script may touch, from its `permissions` block.

use std::path::{Component, Path, PathBuf};

use crate::ast::{Permission, PermissionKind, Target};
use crate::token::Span;

/// A declared permission pointing to something that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPath {
    pub path: String,
    pub span: Span,
}

/// Why an access was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    NotDeclared,
    /// A location no script may ever write, whatever its permissions say.
    Protected(&'static str),
}

/// The accesses a script declared, resolved to real locations on disk.
#[derive(Debug)]
pub struct Permissions {
    script: PathBuf,
    base: PathBuf,
    read: Vec<PathGrant>,
    write: Vec<PathGrant>,
}

#[derive(Debug)]
struct PathGrant {
    target: Target,
    path: PathBuf,
}

impl Permissions {
    /// Resolves the declared paths against the folder holding `script`, so a
    /// script has the same rights wherever it is launched from.
    pub fn new(declared: &[Permission], script: &Path) -> Result<Self, MissingPath> {
        let script = real_path(script);
        let base = script
            .parent()
            .map_or_else(|| real_path(Path::new(".")), Path::to_path_buf);
        let mut read = Vec::new();
        let mut write = Vec::new();
        for permission in declared {
            match &permission.kind {
                // What is read must already exist.
                PermissionKind::Read(access) => {
                    let path = base
                        .join(&access.path)
                        .canonicalize()
                        .map_err(|_| MissingPath {
                            path: access.path.clone(),
                            span: permission.span,
                        })?;
                    read.push(PathGrant {
                        target: access.target,
                        path,
                    });
                }
                // What is written may not exist yet.
                PermissionKind::Write(access) => write.push(PathGrant {
                    target: access.target,
                    path: real_path(&base.join(&access.path)),
                }),
                PermissionKind::Network { .. } | PermissionKind::Env { .. } => {}
            }
        }
        Ok(Self {
            script,
            base,
            read,
            write,
        })
    }

    /// The real location behind `requested`, if the script may read it.
    pub fn resolve_read(&self, requested: &str) -> Result<PathBuf, Denial> {
        let real = real_path(&self.base.join(requested));
        if self.read.iter().any(|grant| grant.allows(&real)) {
            Ok(real)
        } else {
            Err(Denial::NotDeclared)
        }
    }

    /// The real location behind `requested`, if the script may write it.
    pub fn resolve_write(&self, requested: &str) -> Result<PathBuf, Denial> {
        let real = real_path(&self.base.join(requested));
        if real == self.script {
            return Err(Denial::Protected("a script can never rewrite itself"));
        }
        if let Some(reason) = runs_code_later(&real) {
            return Err(Denial::Protected(reason));
        }
        if self.write.iter().any(|grant| grant.allows(&real)) {
            Ok(real)
        } else {
            Err(Denial::NotDeclared)
        }
    }
}

impl PathGrant {
    // `Path::starts_with` compares whole folder names, so `data` does not
    // cover `database.txt`.
    fn allows(&self, path: &Path) -> bool {
        match self.target {
            Target::Folder => path.starts_with(&self.path),
            Target::File => path == self.path,
        }
    }
}

const SHELL_STARTUP_FILES: [&str; 8] = [
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
];

/// Writing to these locations would make another program run code later,
/// outside Slarz and with all of the user's rights. The list catches the
/// classic cases; it cannot be complete, which is why write permissions
/// should point to dedicated output folders.
fn runs_code_later(path: &Path) -> Option<&'static str> {
    let names: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let contains = |sequence: &[&str]| {
        names
            .windows(sequence.len())
            .any(|window| window.iter().zip(sequence).all(|(name, part)| name == part))
    };
    let file_name = names.last().map(String::as_str).unwrap_or_default();

    if contains(&[".git"]) {
        Some("files inside `.git` can make git run code")
    } else if SHELL_STARTUP_FILES.contains(&file_name) || file_name.ends_with("profile.ps1") {
        Some("this file is run by the terminal when it starts")
    } else if contains(&["programs", "startup"]) {
        Some("Windows runs the programs in this folder when it starts")
    } else if contains(&["library", "launchagents"]) || contains(&["library", "launchdaemons"]) {
        Some("macOS runs the programs in this folder when it starts")
    } else if contains(&[".config", "autostart"]) || contains(&[".config", "systemd"]) {
        Some("Linux runs the programs in this folder when it starts")
    } else {
        None
    }
}

/// Where `path` really points: `.`, `..` and symbolic links resolved. This
/// works even when the end of the path does not exist yet, because a missing
/// file must still be judged by the folder it would be in.
fn real_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            return missing
                .iter()
                .rev()
                .fold(real, |mut resolved, component: &Component| {
                    match component {
                        Component::ParentDir => {
                            resolved.pop();
                        }
                        Component::CurDir => {}
                        other => resolved.push(other),
                    }
                    resolved
                });
        }
        match (existing.parent(), existing.components().next_back()) {
            (Some(parent), Some(last)) => {
                missing.push(last);
                existing = parent;
            }
            // Nothing on this path exists: it cannot fall inside any grant.
            _ => return path.to_path_buf(),
        }
    }
}
