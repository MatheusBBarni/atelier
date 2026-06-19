---
status: pending
title: Branch-diff acquisition and diff redaction
type: backend
complexity: medium
dependencies: []
---

# Task 02: Branch-diff acquisition and diff redaction

## Overview
Add the app-side ability to compute the current branch diff (merge-base versus the repo's default branch, including uncommitted working-tree changes) and to redact credential-shaped content from a diff string. This is the "diff-as-data" input the reviewer never fetches itself, and the redaction pass that keeps secrets out of the transcript (ADR-002, ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `fetch_branch_diff(dir) -> Result<Option<BranchDiff>>` in `src/app/git.rs` following the existing subprocess pattern (`tokio::process::Command`, `kill_on_drop`, bounded timeout).
- MUST resolve the comparison base in this order: default branch (`origin/HEAD` → `main` → `master`) merge-base with `HEAD`; fall back to the initial commit when no default branch exists; fall back to `git diff HEAD` (working tree) for detached HEAD or single-commit repos.
- MUST include uncommitted working-tree changes in the diff, and return `None` when there are no changes.
- MUST treat `git diff` exit code 1 (changes present) as success, and a code > 1 as an error.
- `BranchDiff` MUST carry a human-readable `base_label` and a `files` count for the scope statement.
- MUST add `redact_diff(&str) -> String` that masks credential-shaped lines, reusing the secret predicates in `src/file_index.rs`.
- Redaction MUST NOT alter ordinary code lines and MUST mask lines referencing secret file paths/names or credential assignments.

## Subtasks
- [ ] 2.1 Implement default-branch resolution with the documented fallback order.
- [ ] 2.2 Implement merge-base computation and the `git diff <base>` fetch (working tree included).
- [ ] 2.3 Define `BranchDiff { base_label, text, files }` and the empty-diff `None` case.
- [ ] 2.4 Implement `redact_diff` reusing `is_secret_name`/`is_secret_dir` plus credential-assignment patterns.
- [ ] 2.5 Add unit tests for each base-resolution edge case and for redaction.

## Implementation Details
Extend `src/app/git.rs`, mirroring `fetch_git_context`/`run_git` (subprocess, timeout, `kill_on_drop`). Reuse the secret-name/secret-dir predicates from `src/file_index.rs` for `redact_diff`. See TechSpec "System Architecture → Diff acquisition", "Core Interfaces", and ADR-005 (base resolution + truncation context) and ADR-002 (redaction-before-transcript). Do not truncate here — truncation is applied by the workflow in task_05; this task returns the full diff and file count.

### Relevant Files
- `src/app/git.rs` — `fetch_git_context`/`run_git` (~42-89) is the subprocess pattern to extend.
- `src/file_index.rs` — `is_secret_name`/`is_secret_dir` (~305-322) reused by `redact_diff`.

### Dependent Files
- `src/app/mod.rs` — task_05's workflow calls `fetch_branch_diff` and `redact_diff`.

### Related ADRs
- [ADR-005: Merge-base-vs-default-branch diff; truncate-with-note](../adrs/adr-005.md) — base resolution + fallbacks.
- [ADR-002: Redacted findings / diff-as-data](../adrs/adr-002.md) — redaction before content persists.

## Deliverables
- `fetch_branch_diff` + `BranchDiff` in `src/app/git.rs` with deterministic base resolution.
- `redact_diff` reusing existing secret predicates.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the diff path are exercised via FakeRuntime in task_05 **(REQUIRED)**

## Tests
- Unit tests (use temp git repos created in-test):
  - [ ] Branch ahead of `main` by 2 commits + 1 uncommitted edit → diff covers all three, `base_label` names the merge-base/`main`.
  - [ ] Detached HEAD → falls back to `git diff HEAD`, `base_label` reflects the working-tree fallback.
  - [ ] Repo with no `main`/`master`/`origin/HEAD` → falls back to initial commit without error.
  - [ ] First-commit-only repo → returns a working-tree diff or `None` with no panic.
  - [ ] No changes vs base → returns `None`.
  - [ ] `redact_diff` masks a line adding `AWS_SECRET=...`, a `.env` path, and a `*.pem` body; leaves ordinary `fn foo()` lines untouched.
- Integration tests:
  - [ ] (Covered in task_05: redacted diff is embedded as data and reaches the reviewer prompt.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Base resolution is deterministic and every fallback path is covered by a test.
- No credential-shaped line survives `redact_diff`.
