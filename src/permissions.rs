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

/// The accesses a script declared, resolved to real locations on disk.
#[derive(Debug)]
pub struct Permissions {
    base: PathBuf,
    read: Vec<PathGrant>,
}

#[derive(Debug)]
struct PathGrant {
    target: Target,
    path: PathBuf,
}

impl Permissions {
    /// Resolves the declared paths against the folder holding the script, so a
    /// script has the same rights wherever it is launched from.
    pub fn new(declared: &[Permission], script_folder: &Path) -> Result<Self, MissingPath> {
        let base = real_path(script_folder);
        let mut read = Vec::new();
        for permission in declared {
            if let PermissionKind::Read(access) = &permission.kind {
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
        }
        Ok(Self { base, read })
    }

    /// The real location behind `requested`, if the script may read it.
    pub fn resolve_read(&self, requested: &str) -> Option<PathBuf> {
        let real = real_path(&self.base.join(requested));
        self.read
            .iter()
            .any(|grant| grant.allows(&real))
            .then_some(real)
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
