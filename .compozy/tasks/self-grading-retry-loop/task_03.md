---
status: pending
title: GraderVerdict type and exit-code-derived verdict deriver
type: backend
complexity: low
dependencies:
  - task_02
---

# Task 03: GraderVerdict type and exit-code-derived verdict deriver

## Overview
Add the harness-owned `GraderVerdict`/`GradeOutcome` types and a pure `derive_grade_verdict` function that computes Pass/Fail/Skip from a grade step's recorded command exit codes — never from the agent's self-attestation. This is the correctness core of the feature: it makes a fabricated `status: completed` non-load-bearing.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `GraderVerdict { outcome, command, exit_code, critique }` and `GradeOutcome { Pass, Fail, Skip }`, harness-constructed and never deserialized from agent output.
- MUST implement `derive_grade_verdict` as a pure function over the grade step's command results: PASS = at least one canonical command ran AND all canonical commands exited 0; FAIL = at least one canonical command ran AND any exited non-zero; SKIP = no canonical command ran.
- MUST source command identity/exit code from structured command results (the `command_completed` payload / `ActionResult.content`), not from any model-authored text.
- MUST use the task 02 predicate to decide which commands count.
- MUST carry the failing command + exit code + a short failure excerpt as the `critique` on FAIL.

</requirements>

## Subtasks
- [ ] 03.1 Define `GraderVerdict` and `GradeOutcome` near `AgentResult`.
- [ ] 03.2 Define the small command-outcome input type the deriver consumes.
- [ ] 03.3 Implement `derive_grade_verdict` using the canonical-check predicate.
- [ ] 03.4 Populate `critique` from the failing command + exit code on FAIL.
- [ ] 03.5 Cover Pass/Fail/Skip and mixed-command cases with table-driven tests.

## Implementation Details
Pure logic + simple types; no I/O. See TechSpec "Core Interfaces" (`GraderVerdict`, `GradeOutcome`, `derive_grade_verdict`) and "Build Order" step 3. The verdict-grounding findings in `_research-techspec.json` show the `command_completed` payload shape (`{command, exit_code, status}`) the deriver reads.

### Relevant Files
- `src/orchestrator/mod.rs` — add the new types near `AgentResult` (~:147-160); reuse `serde` derives consistent with the surrounding types.
- `src/actions/mod.rs` — the task 02 `is_canonical_verification_command` predicate.

### Dependent Files
- `src/app/mod.rs` — task 05's executor calls `derive_grade_verdict` and emits the verdict.
- `src/app/chat/projection.rs` — task 04 serializes the verdict into the grade event payload.

### Related ADRs
- [ADR-004: Harness-derived verdict from canonical-check exit codes](../adrs/adr-004.md) — directly implemented by this task.
- [ADR-001: Externally-grounded auto-verification loop](../adrs/adr-001.md) — grounding rule the verdict enforces.

## Deliverables
- `GraderVerdict`/`GradeOutcome` types and a pure, well-tested `derive_grade_verdict`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests are covered downstream via task 05; this task's verification is exhaustive unit tests **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `[cargo test → 0]` → Pass with `command = "cargo test"`, `exit_code = 0`.
  - [ ] `[cargo test → 1]` → Fail with `critique` carrying the failing command + exit code.
  - [ ] `[echo hi → 0]` only → Skip (non-canonical command ignored).
  - [ ] `[]` (no commands) → Skip.
  - [ ] `[cargo fmt → 0, cargo test → 1]` → Fail (any canonical non-zero fails).
  - [ ] `[cargo clippy → 0, cargo test → 0]` → Pass.
- Integration tests:
  - [ ] Exercised end-to-end in task 05 (deriver fed real `command_completed` events).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Verdict is computed solely from structured command exit codes
- A fabricated `AgentResult.status`/`verification` cannot produce a PASS
