//! File index for the `@`-mention dropdown.
//!
//! This is the only component that touches the filesystem on behalf of the
//! picker. [`FileIndex::walk`] performs a `.gitignore`-aware walk of the
//! working directory (via the `ignore` crate), enforcing the security
//! guardrails the PRD requires: a secret-name denylist, a build/dependency
//! force-exclude set, symlink rejection, and a working-directory pin so no
//! candidate ever resolves outside the root. The walk is synchronous and is
//! driven off the draw thread by the TUI worker (ADR-003); callers receive an
//! owned `Vec<FileEntry>`.

use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;

/// Directory names pruned from the walk regardless of `.gitignore` — VCS
/// metadata plus build/dependency noise. Mirrors `codemap`'s `EXCLUDED_DIRS`
/// (the task requires at minimum `.multiagent`, `target`, and `node_modules`).
const FORCE_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".multiagent",
    ".next",
    ".pytest_cache",
    ".ruff_cache",
    ".mypy_cache",
    ".turbo",
    ".venv",
    "__pycache__",
    "build",
    "coverage",
    "deps",
    "dist",
    "node_modules",
    "Pods",
    "target",
    "vendor",
    "venv",
];

/// Directory names that conventionally hold credentials; pruned by name so none
/// of their contents are ever surfaced.
const SECRET_DIRS: &[&str] = &[".ssh", ".aws"];

/// A single walked candidate. `FileEntry` is the structured "validated
/// reference" ADR-001 called for: a real path that exists under the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Forward-slashed path relative to the working directory.
    pub rel_path: String,
    /// Whether this entry is a directory (folders are referenceable values).
    pub is_dir: bool,
    /// Last-modified time, used for recency ranking.
    pub mtime: SystemTime,
    /// Path component count, used for shallow-first ranking.
    pub depth: usize,
}

/// Namespace for the file-index operations. The walk and (in a later task) the
/// fuzzy query hang off this type so the filesystem-touching logic stays out of
/// the TUI module (ADR-005). It carries no state because the cached entries
/// live on `TuiUiState` — `nucleo_matcher::Matcher` is neither `Clone` nor
/// `Eq`, so a stateful index could not be held there.
pub struct FileIndex;

impl FileIndex {
    /// Walk `root` and return every file and folder that is a valid candidate.
    ///
    /// Off-thread by contract (the caller runs this in `spawn_blocking`). The
    /// walk honors `.gitignore` even when `root` is not a git repository
    /// (`require_git(false)`), prunes the force-exclude and secret directories,
    /// drops secret-named files, never follows symlinks, and rejects anything
    /// whose canonical path escapes the root. Unreadable subtrees are skipped
    /// rather than aborting the whole walk; a fully failed walk yields an empty
    /// `Vec`.
    pub fn walk(root: &Path) -> Vec<FileEntry> {
        // Canonicalize the root once so containment checks compare like-for-like
        // (on macOS, tempdirs live under a `/var -> /private/var` symlink).
        let canonical_root = match root.canonicalize() {
            Ok(path) => path,
            Err(_) => return Vec::new(),
        };

        let mut entries = Vec::new();
        let walker = WalkBuilder::new(root)
            // Include dotfiles (e.g. `.github/`); secrets are excluded by name.
            .hidden(false)
            // Never traverse a symlink target.
            .follow_links(false)
            // Honor `.gitignore` even outside a git repo (PRD open question).
            .require_git(false)
            .filter_entry(|entry| {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if !is_dir {
                    return true;
                }
                let name = entry.file_name().to_string_lossy();
                !(is_force_excluded_dir(&name) || is_secret_dir(&name))
            })
            .build();

        for result in walker {
            let Ok(dir_entry) = result else {
                // Permission denied on a subtree, etc. — skip and keep walking.
                continue;
            };
            // Skip the root itself (depth 0).
            if dir_entry.depth() == 0 {
                continue;
            }
            let Some(file_type) = dir_entry.file_type() else {
                continue;
            };
            // Reject symlinks outright (do not follow, do not list).
            if file_type.is_symlink() {
                continue;
            }

            let path = dir_entry.path();
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            // Defense in depth: never let a `..` component through.
            if rel
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                continue;
            }
            let rel_path = rel.to_string_lossy().replace('\\', "/");
            if rel_path.is_empty() {
                continue;
            }

            let name = dir_entry.file_name().to_string_lossy();
            if is_secret_name(&name) {
                continue;
            }

            // Reject any candidate whose canonical path escapes the root.
            match path.canonicalize() {
                Ok(canonical) if canonical.starts_with(&canonical_root) => {}
                _ => continue,
            }

            let is_dir = file_type.is_dir();
            let depth = rel_path.split('/').count();
            let mtime = dir_entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(UNIX_EPOCH);

            entries.push(FileEntry {
                rel_path,
                is_dir,
                mtime,
                depth,
            });
        }

        entries
    }
}

fn is_force_excluded_dir(name: &str) -> bool {
    FORCE_EXCLUDED_DIRS.contains(&name)
}

fn is_secret_dir(name: &str) -> bool {
    SECRET_DIRS.contains(&name)
}

/// A static, best-effort secret-name denylist (`.env*`, `*.pem`, `*.key`,
/// `id_rsa*`). Combined with the working-dir pin and symlink rejection, this is
/// the primary guard against surfacing a sensitive filename by name.
fn is_secret_name(name: &str) -> bool {
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use tempfile::tempdir;

    /// Build a representative tree: tracked sources, a `.gitignore` with an
    /// ignored log, build/dependency noise, secret files, and nested folders.
    fn seed_tree(root: &Path) {
        fs::create_dir_all(root.join("src/runtime")).unwrap();
        fs::create_dir_all(root.join("src/tui")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("node_modules/dep")).unwrap();
        fs::create_dir_all(root.join(".ssh")).unwrap();

        fs::write(root.join(".gitignore"), "ignored.log\n").unwrap();
        fs::write(root.join("README.md"), "readme").unwrap();
        fs::write(root.join("src/runtime/claude.rs"), "fn main() {}").unwrap();
        fs::write(root.join("src/tui/mod.rs"), "fn main() {}").unwrap();
        fs::write(root.join("ignored.log"), "noise").unwrap();
        fs::write(root.join("target/debug/app"), "bin").unwrap();
        fs::write(root.join("node_modules/dep/index.js"), "js").unwrap();

        // Secret files (denylist) and a secret directory.
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        fs::write(root.join("server.pem"), "key").unwrap();
        fs::write(root.join("id_rsa"), "key").unwrap();
        fs::write(root.join(".ssh/known_hosts"), "host").unwrap();
    }

    fn rel_paths(entries: &[FileEntry]) -> BTreeSet<String> {
        entries.iter().map(|entry| entry.rel_path.clone()).collect()
    }

    #[test]
    fn walk_returns_files_and_nested_folders() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let entries = FileIndex::walk(dir.path());
        let paths = rel_paths(&entries);

        assert!(paths.contains("README.md"));
        assert!(paths.contains("src"));
        assert!(paths.contains("src/runtime"));
        assert!(paths.contains("src/runtime/claude.rs"));
        assert!(paths.contains("src/tui/mod.rs"));
    }

    #[test]
    fn walk_excludes_gitignored_paths() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.contains("ignored.log"));
    }

    #[test]
    fn walk_excludes_build_and_dependency_dirs() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.iter().any(|p| p.starts_with("target")));
        assert!(!paths.iter().any(|p| p.starts_with("node_modules")));
    }

    #[test]
    fn walk_excludes_secret_files_and_dirs() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.contains(".env"));
        assert!(!paths.contains("server.pem"));
        assert!(!paths.contains("id_rsa"));
        assert!(!paths.iter().any(|p| p.starts_with(".ssh")));
    }

    #[cfg(unix)]
    #[test]
    fn walk_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        // A symlink pointing inside the tree: not followed, not listed.
        symlink(dir.path().join("README.md"), dir.path().join("link.md")).unwrap();
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.contains("link.md"));
    }

    #[cfg(unix)]
    #[test]
    fn walk_rejects_symlink_resolving_outside_root() {
        use std::os::unix::fs::symlink;

        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "outside").unwrap();

        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        // A symlink whose target is outside the root must never appear.
        symlink(outside.path(), dir.path().join("escape")).unwrap();
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.iter().any(|p| p.starts_with("escape")));
    }

    #[test]
    fn walk_populates_metadata_and_forward_slashes() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let entries = FileIndex::walk(dir.path());

        let nested = entries
            .iter()
            .find(|entry| entry.rel_path == "src/tui/mod.rs")
            .expect("nested file present");
        assert!(!nested.is_dir);
        assert_eq!(nested.depth, 3);
        assert!(nested.rel_path.contains('/'));
        assert!(!nested.rel_path.contains('\\'));

        let folder = entries
            .iter()
            .find(|entry| entry.rel_path == "src")
            .expect("folder present");
        assert!(folder.is_dir);
        assert_eq!(folder.depth, 1);

        // mtime is populated (at or after the epoch).
        assert!(nested.mtime >= UNIX_EPOCH);
    }

    #[test]
    fn walk_of_missing_root_is_empty() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(FileIndex::walk(&missing).is_empty());
    }

    #[test]
    fn walk_integration_excludes_all_unsafe_entries() {
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let paths = rel_paths(&FileIndex::walk(dir.path()));

        // Every ignored / secret / noise entry is absent; real sources present.
        for forbidden in [
            "ignored.log",
            ".env",
            "server.pem",
            "id_rsa",
            ".ssh",
            ".ssh/known_hosts",
            "target",
            "target/debug/app",
            "node_modules",
        ] {
            assert!(!paths.contains(forbidden), "{forbidden} should be excluded");
        }
        assert!(paths.contains("src/runtime/claude.rs"));
        assert!(paths.contains("README.md"));
    }
}
