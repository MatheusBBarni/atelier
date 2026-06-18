---
status: completed
title: CI wiring — mirror-equality check + skill tests
type: infra
complexity: low
dependencies:
  - task_05
  - task_06
---

# CI wiring — mirror-equality check + skill tests

## Overview
Wire the skill's guards into CI so config-schema drift, mirror drift, or a non-loading preset fails the build. This runs the `check:skills` mirror-equality gate (task_05) and ensures the `tests/atelier_config_skill.rs` suite (task_06) executes in the existing Rust test gate — making the PRD's accuracy guarantees enforced on every change.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a step to `.github/workflows/release.yml` that runs the mirror-equality check (`npm --prefix npm run check:skills`, task_05) and fails the workflow on drift, alongside the existing `check:versions`/`check:targets` npm checks.
- MUST ensure `tests/atelier_config_skill.rs` (task_06) runs in the existing `cargo test --locked` gate (no separate exclusion); confirm it is picked up by the release workflow's test job.
- MUST keep the new CI steps consistent with the documented pre-commit gate (`cargo fmt --check && cargo clippy --all-targets && cargo test --locked`) so local and CI agree; add the skills check to that gate's documentation if the repo documents it.
- MUST NOT run any LLM/eval test in CI (those stay `#[ignore]`d/env-gated per ADR-005).
- SHOULD place the mirror-equality check early (fast-fail) next to the other npm `check:*` steps.
</requirements>

## Subtasks
- [x] 9.1 Add the `check:skills` mirror-equality step to `release.yml` (next to `check:versions`).
- [x] 9.2 Confirm `tests/atelier_config_skill.rs` runs under the existing `cargo test --locked` job (no opt-out).
- [x] 9.3 Keep the pre-commit gate docs (CLAUDE.md / contributing notes) consistent with the added check, if documented.
- [x] 9.4 Verify the workflow is green end-to-end with the skill present and synced.

## Implementation Note
`check:skills` added in two places in `release.yml`: the `validate-release` job (early fast-fail,
next to `check:targets`/`check:versions`) and the assemble/pack job. `cargo test --locked` picks up
`tests/atelier_config_skill.rs` automatically (no exclusion). `CLAUDE.md`'s documented pre-commit gate
now appends `npm --prefix npm run check:skills`. Verified locally: `check:skills` exits 0 when synced
(and 1 on drift, per task_05), `cargo test --locked --test atelier_config_skill` = 6/6, release.yml is
valid YAML. CI's clean HOME makes the local-only `reload_skills_…` environmental failure (see task_07
follow-up) a non-issue there.

## Implementation Details
Edit `.github/workflows/release.yml` — add the `check:skills` invocation near the existing npm checks (`check:versions` ~169, `check:targets`) and rely on the existing `cargo test --locked` step (~182) to run the new test file. The mirror-equality CI check is the enforcement half of ADR-004; the test suite is ADR-005. See TechSpec "Impact Analysis (CI)" and "Development Sequencing" step 5.

### Relevant Files
- `.github/workflows/release.yml` — npm `check:*` steps (~169) and the `cargo test --locked` gate (~182).
- `npm/package.json` — the `check:skills` script (task_05).
- `tests/atelier_config_skill.rs` — the suite to run (task_06).

### Dependent Files
- None — CI configuration only.

### Related ADRs
- [ADR-004: CI mirror-equality check](../adrs/adr-004.md)
- [ADR-005: Deterministic skill tests gate every change](../adrs/adr-005.md)

## Deliverables
- `release.yml` runs `check:skills` (mirror equality) and the `atelier_config_skill` tests; both must pass for a green build.
- Tests **(REQUIRED)**: the CI gate itself is validated by the underlying task_05 `--check` and task_06 suite passing locally (`cargo test --locked` + `npm run check:skills`); document the verification.

## Tests
- Unit tests:
  - [ ] (Local proxy for CI) `npm --prefix npm run check:skills` exits 0 when synced and non-zero on an injected mirror diff. (task_05 script behavior, exercised here)
- Integration tests:
  - [ ] `cargo test --locked` includes and passes `tests/atelier_config_skill.rs` (the suite is not excluded by the workflow).
- Test coverage target: >=80% (of the CI-gated invariants)
- All tests must pass

## Success Criteria
- All tests passing
- A schema/mirror/preset regression fails CI; the full pre-commit gate (`cargo fmt --check && cargo clippy --all-targets && cargo test --locked`) plus `check:skills` is green.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
