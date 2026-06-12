---
status: pending
title: Add ignore and nucleo-matcher dependencies
type: chore
complexity: low
dependencies: []
---

# Task 01: Add ignore and nucleo-matcher dependencies

## Overview
Add the two crates the file index depends on — `ignore` (ripgrep's gitignore-aware walker) and `nucleo-matcher` (fuzzy scoring) — to `Cargo.toml`. This unblocks the new `src/file_index.rs` module and keeps the rest of the feature buildable.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ignore` to `[dependencies]` in `Cargo.toml`.
- MUST add `nucleo-matcher` to `[dependencies]` in `Cargo.toml`.
- MUST NOT add new `tokio` features — `fs`, `rt-multi-thread`, `sync`, and `time` are already enabled and are sufficient.
- MUST keep `cargo build` and `cargo test` green after the change (no unused-dependency hard failure, since later tasks consume the crates).
</requirements>

## Subtasks
- [ ] 1.1 Add the `ignore` crate dependency.
- [ ] 1.2 Add the `nucleo-matcher` crate dependency.
- [ ] 1.3 Confirm the workspace builds and the lockfile resolves both crates.

## Implementation Details
Modify `Cargo.toml` `[dependencies]` only. See TechSpec "Technical Dependencies" — both crates are net-new and require no new tokio features. Pin to current stable versions consistent with the repo's existing version style.

### Relevant Files
- `Cargo.toml` — the dependency manifest where both crates are declared.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Technical Dependencies" names the crates and confirms tokio features are already present.

### Dependent Files
- `src/file_index.rs` — (created in task_02) will use `ignore::WalkBuilder` and `nucleo_matcher`.
- `src/tui/mod.rs` — (task_04) the background walk uses `tokio::task::spawn_blocking`, already available.

### Related ADRs
- [ADR-001: Scope @-Mention File Dropdown V1](../adrs/adr-001.md) — selects the `ignore` walker.
- [ADR-004: Fuzzy Matching via nucleo-matcher](../adrs/adr-004.md) — selects `nucleo-matcher` over the full `nucleo` worker.

## Deliverables
- `ignore` and `nucleo-matcher` present in `Cargo.toml` `[dependencies]`, with `Cargo.lock` updated.
- Build smoke verification **(REQUIRED)**.
- Import smoke test confirming both crates are usable **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Build smoke: `cargo build` succeeds with both crates resolved in `Cargo.lock`.
  - [ ] Import smoke: a throwaway item referencing `ignore::WalkBuilder` and `nucleo_matcher::Matcher` compiles (removed once task_02/03 use them for real).
- Integration tests:
  - [ ] `cargo test` runs to completion (no link/resolution failure introduced).
- Test coverage target: >=80% (configuration task; functional coverage is delivered by task_02 and task_03 which exercise the crates).
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80% (no new logic paths introduced here; covered by dependent tasks)
- `ignore` and `nucleo-matcher` resolve and the workspace builds
