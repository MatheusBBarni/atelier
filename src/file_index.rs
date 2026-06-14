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

use std::cmp::Ordering;
use std::path::{Component, Path};
use std::sync::atomic::{self, AtomicBool};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Directory names pruned from the walk regardless of `.gitignore` — VCS
/// metadata plus build/dependency noise. Mirrors `codemap`'s `EXCLUDED_DIRS`
/// (the task requires at minimum `.atelier`, `target`, and `node_modules`).
const FORCE_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".atelier",
    ".multiagent", // legacy data root pre-rename; keep old session logs out of the picker
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

/// A ranked, highlight-annotated suggestion handed to the dropdown. Carries the
/// matched character offsets so the renderer can emphasize them (ADR-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSuggestion {
    /// Forward-slashed path relative to the working directory.
    pub rel_path: String,
    /// Whether this entry is a directory (folders render a trailing `/`).
    pub is_dir: bool,
    /// Char offsets into `rel_path` that the fuzzy match covered. Empty for the
    /// recents (empty-query) listing, where nothing is highlighted.
    pub match_indices: Vec<u32>,
}

/// Namespace for the file-index operations. The walk and the fuzzy query hang
/// off this type so the filesystem-touching logic stays out of the TUI module
/// (ADR-005). It carries no state because the cached entries live on
/// `TuiUiState` — `nucleo_matcher::Matcher` is neither `Clone` nor `Eq`, so a
/// stateful index could not be held there.
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
        Self::walk_cancellable(root, &AtomicBool::new(false))
    }

    /// Like [`FileIndex::walk`], but aborts the traversal as soon as `cancel`
    /// is set. The TUI hands in a shutdown flag so an in-flight background walk
    /// (run on a `spawn_blocking` thread) stops promptly on quit instead of
    /// keeping that blocking thread — and therefore process exit — alive on a
    /// large workspace. The partial result is discarded by the caller, so
    /// bailing mid-walk is safe.
    pub fn walk_cancellable(root: &Path, cancel: &AtomicBool) -> Vec<FileEntry> {
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
            // Stop the moment a shutdown is requested so the blocking thread
            // running this walk frees promptly.
            if cancel.load(atomic::Ordering::Relaxed) {
                break;
            }
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

    /// Rank `entries` for `query` and return up to `limit` suggestions.
    ///
    /// In-memory and synchronous — run per keystroke over the cached entries.
    /// An empty query lists recents (most-recently-modified, then shallower,
    /// then alphabetical). A non-empty query is scored with `nucleo-matcher`
    /// (path-aware, case-insensitive) and ordered by fuzzy score descending,
    /// then shallower path, then most-recently-modified, then alphabetical
    /// (ADR-004). Non-matching entries are dropped; matched char offsets are
    /// returned for highlighting. A single `Matcher` is reused across the whole
    /// candidate list.
    pub fn query(entries: &[FileEntry], query: &str, limit: usize) -> Vec<FileSuggestion> {
        if query.is_empty() {
            let mut recents: Vec<&FileEntry> = entries.iter().collect();
            recents.sort_by(|a, b| recents_order(a, b));
            return recents
                .into_iter()
                .take(limit)
                .map(|entry| FileSuggestion {
                    rel_path: entry.rel_path.clone(),
                    is_dir: entry.is_dir,
                    match_indices: Vec::new(),
                })
                .collect();
        }

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<ScoredEntry<'_>> = Vec::new();
        let mut haystack_buf: Vec<char> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for entry in entries {
            haystack_buf.clear();
            indices.clear();
            let haystack = Utf32Str::new(&entry.rel_path, &mut haystack_buf);
            if let Some(score) = atom.indices(haystack, &mut matcher, &mut indices) {
                indices.sort_unstable();
                indices.dedup();
                scored.push(ScoredEntry {
                    score,
                    entry,
                    match_indices: indices.clone(),
                });
            }
        }

        scored.sort_by(|a, b| query_order(a, b));
        scored
            .into_iter()
            .take(limit)
            .map(|scored| FileSuggestion {
                rel_path: scored.entry.rel_path.clone(),
                is_dir: scored.entry.is_dir,
                // `nucleo` builds its haystack from grapheme clusters, keeping
                // only the first codepoint of each, so the offsets it returns
                // line up with our per-`char` indices only for ASCII paths. For
                // a path with a combining mark or a multi-codepoint emoji the
                // offsets would highlight the wrong characters, so drop the
                // highlights there — the match (and ranking) is still correct.
                match_indices: if scored.entry.rel_path.is_ascii() {
                    scored.match_indices
                } else {
                    Vec::new()
                },
            })
            .collect()
    }
}

/// A candidate that matched the query, with its fuzzy score and highlight
/// offsets, awaiting the final ranking sort.
struct ScoredEntry<'a> {
    score: u16,
    entry: &'a FileEntry,
    match_indices: Vec<u32>,
}

/// Recents ordering (empty query): most-recently-modified, then shallower path,
/// then alphabetical.
fn recents_order(a: &FileEntry, b: &FileEntry) -> Ordering {
    b.mtime
        .cmp(&a.mtime)
        .then_with(|| a.depth.cmp(&b.depth))
        .then_with(|| a.rel_path.cmp(&b.rel_path))
}

/// Non-empty query ordering: fuzzy score descending, then shallower path, then
/// most-recently-modified, then alphabetical.
fn query_order(a: &ScoredEntry<'_>, b: &ScoredEntry<'_>) -> Ordering {
    b.score
        .cmp(&a.score)
        .then_with(|| a.entry.depth.cmp(&b.entry.depth))
        .then_with(|| b.entry.mtime.cmp(&a.entry.mtime))
        .then_with(|| a.entry.rel_path.cmp(&b.entry.rel_path))
}

fn is_force_excluded_dir(name: &str) -> bool {
    FORCE_EXCLUDED_DIRS.contains(&name)
}

fn is_secret_dir(name: &str) -> bool {
    SECRET_DIRS.iter().any(|dir| name.eq_ignore_ascii_case(dir))
}

/// A static, best-effort secret-name denylist (`.env*`, `*.pem`, `*.key`,
/// `id_rsa*`). Combined with the working-dir pin and symlink rejection, this is
/// the primary guard against surfacing a sensitive filename by name.
///
/// Matched case-insensitively: the default filesystems on macOS (APFS) and
/// Windows (NTFS) are case-insensitive, so a file stored as `.ENV` or `ID_RSA`
/// is the same secret and must be excluded just like its lowercase form.
fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(".env")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.starts_with("id_rsa")
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
    fn walk_excludes_atelier_private_dir() {
        // `.atelier` is the harness's private, durable session-history dir; it must
        // never surface in the file index (FORCE_EXCLUDED_DIRS). Guards the exclusion
        // against accidental removal from the list.
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        fs::create_dir_all(dir.path().join(".atelier/sessions/abc")).unwrap();
        fs::write(
            dir.path().join(".atelier/sessions/abc/events.jsonl"),
            "{}\n",
        )
        .unwrap();
        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.iter().any(|p| p.starts_with(".atelier")));
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

    #[test]
    fn walk_excludes_secret_files_and_dirs_case_insensitively() {
        // macOS/Windows default filesystems are case-insensitive, so an
        // uppercased secret name is the same secret and must still be pruned.
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".SSH")).unwrap();
        fs::write(dir.path().join(".ENV"), "SECRET=1").unwrap();
        fs::write(dir.path().join("Server.PEM"), "key").unwrap();
        fs::write(dir.path().join("private.KEY"), "key").unwrap();
        fs::write(dir.path().join("ID_RSA"), "key").unwrap();
        fs::write(dir.path().join(".SSH/known_hosts"), "host").unwrap();
        fs::write(dir.path().join("keep.rs"), "fn main() {}").unwrap();

        let paths = rel_paths(&FileIndex::walk(dir.path()));
        assert!(!paths.contains(".ENV"));
        assert!(!paths.contains("Server.PEM"));
        assert!(!paths.contains("private.KEY"));
        assert!(!paths.contains("ID_RSA"));
        assert!(!paths.iter().any(|p| p.starts_with(".SSH")));
        // A non-secret file alongside them is still surfaced.
        assert!(paths.contains("keep.rs"));
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
    fn walk_cancelled_before_start_yields_empty() {
        // A pre-set cancellation flag stops the walk before it lists anything,
        // so the blocking thread returns immediately on shutdown.
        let dir = tempdir().unwrap();
        seed_tree(dir.path());
        let cancel = AtomicBool::new(true);
        assert!(FileIndex::walk_cancellable(dir.path(), &cancel).is_empty());
        // The uncancelled walk over the same tree is non-empty (control).
        assert!(!FileIndex::walk(dir.path()).is_empty());
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

    fn entry(rel_path: &str, is_dir: bool, mtime_secs: u64) -> FileEntry {
        FileEntry {
            rel_path: rel_path.to_string(),
            is_dir,
            mtime: UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs),
            depth: rel_path.split('/').count(),
        }
    }

    fn paths_of(suggestions: &[FileSuggestion]) -> Vec<String> {
        suggestions.iter().map(|s| s.rel_path.clone()).collect()
    }

    #[test]
    fn empty_query_lists_recents_most_recent_first() {
        let entries = vec![
            entry("old.rs", false, 10),
            entry("newest.rs", false, 30),
            entry("middle.rs", false, 20),
        ];
        let suggestions = FileIndex::query(&entries, "", 6);
        assert_eq!(paths_of(&suggestions), ["newest.rs", "middle.rs", "old.rs"]);
        // Recents carry no highlight offsets.
        assert!(suggestions.iter().all(|s| s.match_indices.is_empty()));
    }

    #[test]
    fn empty_query_respects_limit() {
        let entries = vec![
            entry("a.rs", false, 1),
            entry("b.rs", false, 2),
            entry("c.rs", false, 3),
        ];
        assert_eq!(FileIndex::query(&entries, "", 2).len(), 2);
    }

    #[test]
    fn fuzzy_query_ranks_shallow_relevant_above_deep_coincidental() {
        // `rcm` hits `runtime`/`claude` at path boundaries in the shallow path,
        // but only scattered mid-word characters in the deeper path — even
        // though the deeper path is more recent, the stronger match wins.
        let entries = vec![
            entry("src/runtime/claude.rs", false, 10),
            entry("lib/core/structures/dynamic.rs", false, 99),
        ];
        let suggestions = FileIndex::query(&entries, "rcm", 6);
        assert_eq!(
            suggestions.first().map(|s| s.rel_path.as_str()),
            Some("src/runtime/claude.rs")
        );
    }

    #[test]
    fn equal_score_ranks_shallower_path_first() {
        // Identical matched basename at a delimiter boundary in both → equal
        // fuzzy score; the depth tiebreak must surface the shallower path.
        let entries = vec![
            entry("b/c/foo.txt", false, 50),
            entry("a/foo.txt", false, 50),
        ];
        let suggestions = FileIndex::query(&entries, "foo.txt", 6);
        assert_eq!(paths_of(&suggestions), ["a/foo.txt", "b/c/foo.txt"]);
    }

    #[test]
    fn result_count_never_exceeds_limit() {
        let entries: Vec<FileEntry> = (0..20)
            .map(|i| entry(&format!("src/mod_{i}.rs"), false, i))
            .collect();
        assert_eq!(FileIndex::query(&entries, "mod", 6).len(), 6);
    }

    #[test]
    fn match_indices_identify_the_matched_characters() {
        let entries = vec![entry("src/tui/mod.rs", false, 10)];
        let suggestions = FileIndex::query(&entries, "tuimod", 6);
        let suggestion = suggestions.first().expect("a match");
        let chars: Vec<char> = suggestion.rel_path.chars().collect();
        let matched: String = suggestion
            .match_indices
            .iter()
            .map(|&i| chars[i as usize])
            .collect();
        assert_eq!(matched.to_lowercase(), "tuimod");
        // The single occurrence of each character makes the offsets exact.
        assert_eq!(suggestion.match_indices, vec![4, 5, 6, 8, 9, 10]);
    }

    #[test]
    fn query_drops_highlights_for_non_ascii_paths() {
        // `nucleo`'s match offsets are taken over grapheme-collapsed codepoints,
        // so for a non-ASCII path they would not line up with our `char`
        // indices. The match must still rank, but with no (misaligned)
        // highlights rather than emphasizing the wrong characters.
        let entries = vec![entry("café/main.rs", false, 10)];
        let suggestions = FileIndex::query(&entries, "main", 6);
        let suggestion = suggestions.first().expect("non-ascii path still matches");
        assert_eq!(suggestion.rel_path, "café/main.rs");
        assert!(
            suggestion.match_indices.is_empty(),
            "highlights are suppressed for non-ascii paths"
        );

        // An ASCII path with the same query keeps its highlights (control).
        let ascii = vec![entry("core/main.rs", false, 10)];
        let ascii_hit = &FileIndex::query(&ascii, "main", 6)[0];
        assert!(!ascii_hit.match_indices.is_empty());
    }

    #[test]
    fn query_with_no_match_returns_empty() {
        let entries = vec![entry("src/tui/mod.rs", false, 10)];
        assert!(FileIndex::query(&entries, "zzzzzz", 6).is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let entries = vec![entry("src/runtime/claude.rs", false, 10)];
        let suggestions = FileIndex::query(&entries, "CLAUDE", 6);
        assert_eq!(
            suggestions.first().map(|s| s.rel_path.as_str()),
            Some("src/runtime/claude.rs")
        );
    }

    #[test]
    fn query_integration_orders_caps_and_highlights() {
        let entries = vec![
            entry("src/tui/mod.rs", false, 30),
            entry("src/runtime/mod.rs", false, 20),
            entry("src/config/mod.rs", false, 10),
            entry("README.md", false, 5),
            entry("docs/guide/intro.md", false, 1),
        ];
        let suggestions = FileIndex::query(&entries, "mod", 2);
        // Capped to the limit, every result matched, and highlights present.
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions
            .iter()
            .all(|s| s.rel_path.contains("mod.rs") && !s.match_indices.is_empty()));
    }
}
