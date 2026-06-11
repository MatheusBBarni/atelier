//! Git repository context (repo name + branch) for the status footer and the
//! welcome facts box.
//!
//! A short `git rev-parse` subprocess is the entire mechanism (ADR-001): it
//! collapses every edge case — bare repos, worktrees, submodules, detached HEAD
//! — into one fallback contract. Any failure (non-zero exit, timeout, missing
//! binary, not a repo) degrades to `None` so a non-git working directory never
//! surfaces an error. Raw `.git/HEAD` parsing is intentionally avoided: in
//! worktrees/submodules `.git` is a file, and detached HEAD yields a raw SHA
//! (ADR-006).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Subprocess timeout. A slow filesystem (e.g. a network mount) yields `None`
/// rather than blocking the poll; the next tick retries.
const GIT_TIMEOUT: Duration = Duration::from_millis(500);

/// Repo name and branch shown to the user. `PartialEq` is the change gate: the
/// poller publishes a state update only when this value differs from the last.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitContext {
    /// `file_name` of `git rev-parse --show-toplevel`.
    pub repo_name: String,
    /// `git rev-parse --abbrev-ref HEAD`, or the short SHA when detached.
    pub branch: String,
}

/// Fetch the git context for `dir`, or `None` on any failure (never errors).
pub async fn fetch_git_context(dir: &Path) -> Option<GitContext> {
    fetch_with_git("git", dir).await
}

/// Inner fetch parameterized by the git executable so tests can exercise the
/// missing-binary path with a name that does not resolve.
async fn fetch_with_git(git_bin: &str, dir: &Path) -> Option<GitContext> {
    let abbrev = run_git(git_bin, dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let abbrev = abbrev.trim();
    // `--abbrev-ref HEAD` reports the literal "HEAD" when detached; fall back to
    // the short SHA so the footer names the commit rather than "HEAD".
    let branch = if abbrev == "HEAD" {
        run_git(git_bin, dir, &["rev-parse", "--short", "HEAD"])
            .await?
            .trim()
            .to_string()
    } else {
        abbrev.to_string()
    };

    let toplevel = run_git(git_bin, dir, &["rev-parse", "--show-toplevel"]).await?;
    let repo_name = Path::new(toplevel.trim())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())?;

    if branch.is_empty() || repo_name.is_empty() {
        return None;
    }
    Some(GitContext { repo_name, branch })
}

/// Run `git <args>` in `dir` with a timeout, returning trimmed-of-nothing stdout
/// on a zero exit, or `None` on non-zero exit, timeout, or spawn failure.
///
/// Mirrors the runtime subprocess pattern (`tokio::process::Command` +
/// `kill_on_drop(true)` + `tokio::select!` timeout, `src/runtime/claude.rs`):
/// on timeout the `wait_with_output` future — which owns the child — is dropped,
/// so `kill_on_drop` reaps it.
async fn run_git(git_bin: &str, dir: &Path, args: &[&str]) -> Option<String> {
    let child = Command::new(git_bin)
        .args(args)
        .current_dir(dir)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let output = tokio::select! {
        _ = tokio::time::sleep(GIT_TIMEOUT) => return None,
        output = child.wait_with_output() => output.ok()?,
    };

    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::tempdir;

    /// Run a git command during test setup, failing loudly on error.
    fn git(dir: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git available in test environment");
        assert!(status.success(), "git {args:?} failed");
    }

    /// Initialize a repo with one commit, then check out `branch`.
    fn init_repo(dir: &Path, branch: &str) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "hello").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "init"]);
        git(dir, &["checkout", "-b", branch]);
    }

    #[tokio::test]
    async fn fetches_branch_and_repo_name_in_a_repo() {
        let dir = tempdir().unwrap();
        init_repo(dir.path(), "feat/x");

        let context = fetch_git_context(dir.path()).await.expect("Some in a repo");
        assert_eq!(context.branch, "feat/x");
        assert_eq!(
            context.repo_name,
            dir.path().file_name().unwrap().to_string_lossy()
        );
    }

    #[tokio::test]
    async fn returns_none_outside_a_repo() {
        let dir = tempdir().unwrap();
        assert!(fetch_git_context(dir.path()).await.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_git_binary_is_missing() {
        let dir = tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        // A command name that does not resolve must degrade to None, not error.
        assert!(fetch_with_git("atelier-no-such-git-binary", dir.path())
            .await
            .is_none());
    }

    #[tokio::test]
    async fn detached_head_yields_a_non_empty_sha_branch() {
        let dir = tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        // Detach HEAD onto the current commit.
        git(dir.path(), &["checkout", "--detach", "HEAD"]);

        let context = fetch_git_context(dir.path())
            .await
            .expect("Some when detached");
        assert!(!context.branch.is_empty());
        assert_ne!(context.branch, "HEAD", "detached resolves to the short SHA");
    }
}
