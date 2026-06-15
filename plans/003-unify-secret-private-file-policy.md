# Plan 003: Apply Secret And Private Path Policy To Model File Actions

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- src/actions/mod.rs src/file_index.rs src/lib.rs`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: `plans/002-close-relative-symlink-escapes.md`
- **Category**: security
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

The file picker hides `.atelier`, build folders, and secret-looking files, but harness file actions enforce a different policy. A read-capable model can request `read_file` for `.env`-style names or `list_files` into private history, and small action contents are stored directly in history while large contents are stored as artifacts. The action layer should enforce the same private/secret path expectations as user-facing discovery surfaces.

## Current State

- File picker policy rejects secret names and symlinks:

```text
src/file_index.rs:89 walk honors `.gitignore`
src/file_index.rs:90 prunes force-exclude and secret directories
src/file_index.rs:91 drops secret-named files, never follows symlinks
src/file_index.rs:316 fn is_secret_name(name: &str) -> bool {
src/file_index.rs:318     lower.starts_with(".env")
src/file_index.rs:319         || lower.ends_with(".pem")
src/file_index.rs:320         || lower.ends_with(".key")
src/file_index.rs:321         || lower.starts_with("id_rsa")
```

- Actions read and return full file content:

```text
src/actions/mod.rs:871 let contents = fs::read_to_string(&resolved)
src/actions/mod.rs:877 Some(json!({ "path": path, "content": contents })),
```

- `list_files` recurses without the private-dir skip used by search:

```text
src/actions/mod.rs:1147 for entry in fs::read_dir(path)...
src/actions/mod.rs:1153 entries.push(display_action_path(...))
src/actions/mod.rs:1155 collect_file_entries(...)
```

- Large action content is persisted as user-content artifacts:

```text
src/app/mod.rs:4878 let bytes = serde_json::to_vec_pretty(content)?;
src/app/mod.rs:4887 "contains_user_content",
```

Repo conventions:
- Actions and capabilities are centralized in `src/actions/mod.rs`.
- File-index filesystem rules live in `src/file_index.rs`; avoid divergent copies if possible.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `rtk cargo test --lib secret` | relevant tests pass |
| Action tests | `rtk cargo test --lib actions` | action tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/actions/mod.rs`
- `src/file_index.rs`
- `src/lib.rs`
- Optional new shared module such as `src/path_policy.rs`

**Out of scope**:
- Do not add config flags to bypass secret/private policy.
- Do not read real `.env`, private key, or session-history contents.
- Do not change session-history storage format in this plan.

## Git Workflow

- Branch: `advisor/003-secret-private-action-policy`
- Commit message example: `fix(actions): block private paths in file actions`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Extract Shared Path Policy

Create a small shared helper module, for example `src/path_policy.rs`, with functions for:
- private/build directory names, including `.atelier`, `.multiagent`, `.git`, `target`, `node_modules`, `.next`, `dist`, `build`,
- secret directory names, including `.ssh` and `.aws`,
- secret file names matching the current picker behavior: `.env*`, `*.pem`, `*.key`, `id_rsa*`, case-insensitive.

Update `src/file_index.rs` to call the shared helpers instead of private copies.

**Verify**: `rtk cargo test --lib file_index` -> file-index tests pass.

### Step 2: Apply Policy To File Actions

In `src/actions/mod.rs`, enforce the shared policy after resolving action paths:
- `read_file` denies secret-named files and paths under private/secret dirs.
- `list_files` skips private/build/secret directories and secret-named files.
- `search_text` keeps skipping private/build dirs and also skips secret dirs and secret-named files.
- `write_file` and `apply_patch` deny secret-named targets by default.

Keep error messages policy-oriented and avoid echoing file contents.

**Verify**: `rtk cargo test --lib secret` -> new and existing secret/path tests pass.

### Step 3: Add Regression Tests

Add tests in `src/actions/mod.rs` covering:
- `read_file` denies `.env`, `.ENV`, `id_rsa`, `secret.pem`, and `secret.key` using harmless dummy contents.
- `list_files` does not return `.atelier`, `.multiagent`, `.ssh`, `.aws`, `.env`, `id_rsa`, `*.pem`, or `*.key`.
- `search_text` does not return matches from those paths.
- Normal files like `README.md` and `src/lib.rs` still work.

Do not use real secret values in fixtures; use dummy strings like `dummy`.

**Verify**: `rtk cargo test --lib actions` -> action tests pass.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Shared policy tests for case-insensitive secret names.
- Action tests for read/list/search/write/patch denial or skipping.
- Existing file-index tests must still pass.

## Done Criteria

- [ ] File picker and action layer use one shared private/secret path policy.
- [ ] Model actions cannot read or write secret-looking paths by default.
- [ ] `list_files` and `search_text` do not surface private runtime history.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- A legitimate current test requires model actions to read `.env`, private keys, or `.atelier` history.
- A shared helper would create a circular module dependency.
- The fix requires changing persisted history format.

## Maintenance Notes

When new private runtime roots or secret file patterns are introduced, update the shared policy and both file-index/action tests in the same PR.

