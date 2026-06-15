# Plan 001: Stop Tracking Runtime History

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- .gitignore .atelier .multiagent .github/workflows`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

Atelier persists prompts, action metadata, provider diagnostics, and artifacts under `.atelier/`. The repo already ignores that directory, but two files under `.atelier/sessions/...` are still tracked in git. Tracked private runtime state can be committed, reviewed, and published accidentally.

## Current State

- `.gitignore` ignores both private runtime roots:

```text
.gitignore:1 /target/
.gitignore:2 /.atelier/
.gitignore:3 /.multiagent/
```

- `README.md` documents `.atelier` as the session-history location:

```text
README.md:222 - `.atelier/sessions/<session-id>/events.jsonl`
README.md:223 - `.atelier/sessions/<session-id>/artifacts/*`
README.md:224 - `.atelier/runs/<run-id>.json`
```

- `rtk git ls-files --stage .atelier .multiagent` currently reports two tracked `.atelier` files. Do not read or quote their contents.

Repo conventions:
- Use `rtk` before shell commands.
- Commit messages use conventional style, for example `fix(actions): ...` and `chore(release): ...`.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Check tracked private files | `rtk git ls-files .atelier .multiagent` | no output after the fix |
| Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- Remove tracked `.atelier/**` and `.multiagent/**` files from the git index only.
- Add a CI guard, preferably `.github/workflows/repo-hygiene.yml`, that fails if private runtime roots are tracked.
- Optionally update `.gitignore` only if a missing private path is discovered.

**Out of scope**:
- Do not read, summarize, copy, or commit the contents of tracked session files.
- Do not delete the user's local `.atelier/` files from disk; use index-only removal.
- Do not rotate credentials yourself. If a credential type is discovered by a private human audit, report location/type only.

## Git Workflow

- Branch: `advisor/001-untrack-runtime-history`
- Commit message example: `chore(repo): stop tracking atelier runtime history`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Remove Private Runtime Files From The Index

Run index-only removal for private roots:

```bash
rtk git rm --cached -r .atelier .multiagent
```

If `.multiagent` has no tracked files, git may report that path as unmatched. That is fine as long as `.atelier` tracked files are removed from the index.

**Verify**: `rtk git ls-files .atelier .multiagent` -> no output.

### Step 2: Add A Tracked-Private-State Guard

Create `.github/workflows/repo-hygiene.yml` with a pull-request and main-push check that runs:

```bash
tracked_private="$(git ls-files .atelier .multiagent)"
test -z "$tracked_private"
```

If the variable is non-empty, print the tracked paths and exit 1. Keep the workflow small; it only needs checkout and this shell step.

**Verify**: `rtk git diff -- .github/workflows/repo-hygiene.yml` -> shows the new workflow with the `git ls-files .atelier .multiagent` guard.

### Step 3: Run The Repo Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; tests pass.

## Test Plan

- No Rust tests are required for index cleanup.
- The CI guard is the regression test. It must fail if a future commit tracks `.atelier/**` or `.multiagent/**`.

## Done Criteria

- [ ] `rtk git ls-files .atelier .multiagent` prints nothing.
- [ ] `.github/workflows/repo-hygiene.yml` exists and checks tracked private roots.
- [ ] No session-file contents appear in commit messages, plans, or logs.
- [ ] `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- You need to read session history contents to complete the plan.
- `git rm --cached` would remove non-private source files.
- The repo intentionally starts tracking a private runtime path in newer docs or code.

## Maintenance Notes

Reviewers should confirm that the change removes files from the index, not the user's local disk. Future private data roots must be added to both `.gitignore` and the hygiene guard.

