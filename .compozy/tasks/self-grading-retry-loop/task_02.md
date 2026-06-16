---
status: pending
title: Extract canonical-verification-command predicate
type: refactor
complexity: low
dependencies: []
---

# Task 02: Extract canonical-verification-command predicate

## Overview
Factor a reusable `is_canonical_verification_command` predicate out of the existing read-only command allowlist so the grading verdict and the auto-approval allowlist share a single definition of "a real check" (`cargo test/check/build/clippy/fmt`). This prevents drift between what auto-approves and what grounds a PASS.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST expose a predicate that returns true for canonical Rust verification commands (`cargo test`, `cargo check`, `cargo build`, `cargo clippy`, `cargo fmt`) matched by prefix on the trimmed/lowercased command.
- MUST be the single source of the canonical-check set: `is_default_read_only_command` and the new predicate MUST share the canonical prefixes (no duplicated literal lists).
- MUST exclude non-verification commands (`echo`, `ls`, `cargo run`) and anything containing shell-control syntax (already disqualified upstream).
- MUST preserve the existing auto-approval behavior of `is_default_read_only_command` unchanged (pure refactor).

</requirements>

## Subtasks
- [ ] 02.1 Identify the canonical verification prefixes inside the read-only allowlist.
- [ ] 02.2 Extract them into a shared predicate consumable by the verdict deriver.
- [ ] 02.3 Re-express `is_default_read_only_command` in terms of the shared set so behavior is identical.
- [ ] 02.4 Add unit tests asserting both positive and negative classification.

## Implementation Details
The canonical prefixes already live in `is_default_read_only_command`. Extract them so task 03's deriver can call the predicate. See TechSpec "Core Interfaces" (`is_canonical_verification_command`) and "Build Order" step 2. The exact allowlist contents and `has_shell_control_syntax` interaction are in the config findings of `_research-techspec.json`.

### Relevant Files
- `src/actions/mod.rs` — `is_default_read_only_command` (~:443-484) holds the `cargo test/check/build/clippy/fmt` prefixes; `has_shell_control_syntax` (~:486-521) disqualifies chained/piped commands.

### Dependent Files
- `src/orchestrator/mod.rs` — task 03's `derive_grade_verdict` consumes the new predicate.

### Related ADRs
- [ADR-004: Harness-derived verdict from canonical-check exit codes](../adrs/adr-004.md) — defines the canonical-check set as the grounding basis.

## Deliverables
- A shared `is_canonical_verification_command` predicate used by both the allowlist and the verdict deriver.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests confirming auto-approval behavior is unchanged **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `cargo test`, `cargo test --all`, `cargo clippy --all-targets`, `cargo fmt --check`, `cargo build`, `cargo check` → true.
  - [ ] `echo hi`, `ls`, `cargo run`, `make test` → false.
  - [ ] `cargo test && cargo clippy` (shell-control) → false / not auto-allowed.
- Integration tests:
  - [ ] A plain `cargo test` action still auto-approves in normal mode (existing allowlist behavior preserved).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The canonical-check set is defined exactly once and shared
- `is_default_read_only_command` behavior is byte-for-byte unchanged
