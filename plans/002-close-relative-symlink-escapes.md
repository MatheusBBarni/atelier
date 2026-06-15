# Plan 002: Close Relative Symlink Escapes In Harness Actions

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- src/actions/mod.rs`
> If `src/actions/mod.rs` changed, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

Harness actions are the boundary that keeps model-requested reads, writes, and patches inside policy. Absolute paths under configured roots already resolve symlinks before authorization, but relative paths are only checked lexically and then joined to the working directory. A symlink inside the workspace can therefore point actions at files outside the intended boundary.

## Current State

- Absolute paths call `canonical_path_within_root`:

```text
src/actions/mod.rs:617 pub fn validate_model_path(path: &str, extra_roots: &[PathBuf]) -> Result<PathBuf> {
src/actions/mod.rs:640 // A lexical prefix match is necessary but not sufficient...
src/actions/mod.rs:645 if candidate.starts_with(root) && canonical_path_within_root(candidate, root) {
```

- Relative paths pass lexical checks and are returned unchanged:

```text
src/actions/mod.rs:652 for component in candidate.components() {
src/actions/mod.rs:654     Component::ParentDir => bail!("path traversal is not allowed: {path}"),
src/actions/mod.rs:661 Ok(candidate.to_path_buf())
```

- The join happens later without containment validation:

```text
src/actions/mod.rs:1111 let validated = validate_model_path(path, extra_roots)?;
src/actions/mod.rs:1115     working_directory.join(validated)
src/actions/mod.rs:1120 Ok(resolved)
```

Repo conventions:
- Keep action policy centralized in `src/actions/mod.rs`.
- Tests for this module are colocated under `#[cfg(test)]` in `src/actions/mod.rs`.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `rtk cargo test --lib symlink` | relevant tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/actions/mod.rs`

**Out of scope**:
- Do not change `WorkspacePolicy` schema.
- Do not loosen absolute-path restrictions.
- Do not change runtime adapters.

## Git Workflow

- Branch: `advisor/002-close-relative-symlink-escapes`
- Commit message example: `fix(actions): reject relative symlink escapes`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add Failing Characterization Tests

In `src/actions/mod.rs` tests, add Unix-only tests that create:
- a workspace tempdir,
- an outside tempdir with a file,
- a symlink inside the workspace pointing to the outside tempdir.

Cover at least:
- `read_file` on `link/outside.txt` is denied.
- `list_files` on `link` is denied.
- `search_text` on `link` is denied.
- `write_file` on `link/new.txt` is denied.

Use the existing `validate_model_path_rejects_in_root_symlink_escape` test as the style pattern.

**Verify**: `rtk cargo test --lib relative_symlink` -> new tests fail before the implementation.

### Step 2: Enforce Containment In `resolve_action_path`

Update `resolve_action_path` so that relative paths are validated against the canonical working directory after joining. Reuse the deepest-existing-ancestor approach already used by `canonical_path_within_root`, but compare the resolved path against the canonical working directory.

The helper must handle:
- existing read/list/search targets,
- not-yet-created write targets,
- files under symlinked temp roots on macOS.

Do not authorize any path that the existing `validate_model_path` rejected.

**Verify**: `rtk cargo test --lib relative_symlink` -> new tests pass.

### Step 3: Confirm Patch Paths Use The Same Boundary

`apply_unified_diff` already calls `resolve_action_path` when applying patches. Add a test showing a patch target through a workspace symlink is rejected before any file is modified.

**Verify**: `rtk cargo test --lib apply_patch` -> patch tests pass.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Add new `#[cfg(unix)]` tests in `src/actions/mod.rs` for relative symlink escapes on read, list, search, write, and patch.
- Reuse tempdir/symlink style from `validate_model_path_rejects_in_root_symlink_escape`.

## Done Criteria

- [ ] Relative symlink escapes are denied for all action path types.
- [ ] Absolute-path behavior remains covered by existing tests.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- The fix requires changing config schema or default workspace roots.
- The helper rejects normal in-workspace paths under macOS symlinked tempdirs.
- Any non-action module must be changed to make tests pass.

## Maintenance Notes

Future action kinds that accept paths must go through the same resolver. Reviewers should look for duplicated path normalization logic outside `src/actions/mod.rs`.

