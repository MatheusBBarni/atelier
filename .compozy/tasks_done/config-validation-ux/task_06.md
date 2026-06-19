---
status: completed
title: "Document strict and exit-code contract and dogfood in release CI"
type: docs
complexity: low
dependencies:
  - task_05
---

# Task 06: Document strict and exit-code contract and dogfood in release CI

## Overview
Document the new `--strict` flag and the exit-code contract in the README, add a copy-paste CI example, and add a dogfood `atelier --doctor --strict` health-gate step to the release workflow. This closes the adoption-risk gap by making the opt-in flag discoverable and proving it in atelier's own pipeline.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `--strict` to the README CLI flag list and a "Notes" entry stating it is valid only with `--doctor`.
- MUST document the exit-code contract: `0` = healthy, non-zero = unhealthy, "branch on `!= 0`"; codes `1`/`2` reserved (not emitted in V1).
- MUST NOT claim V1 emits a distinct exit code `2` (honest framing per ADR-001).
- MUST add a copy-paste CI example (shell or GitHub Actions step) running `atelier --doctor --strict`.
- MUST add a `--doctor --strict` health-gate step to `.github/workflows/release.yml`, using an available orchestrator runtime.
- SHOULD frame the typo hint as "better hints on errors that already occur," not "typo detection."
</requirements>

## Subtasks
- [x] 6.1 Update the README CLI list and Notes with `--strict` and the exit-code contract.
- [x] 6.2 Add the copy-paste CI snippet to the README.
- [x] 6.3 Add the dogfood `--doctor --strict` step to the release workflow.
- [x] 6.4 Verify the docs link-check passes and the workflow YAML is valid.

## Implementation Details
See PRD "User Experience" and "High-Level Technical Constraints", and ADR-001/ADR-002. The README CLI section is ~86-112 (flag list + "Notes:" block); the release workflow mirrors the pre-commit gate (`cargo fmt --check && cargo clippy --all-targets && cargo test --locked`). This task touches documentation and CI configuration only — no crate source changes.

### Relevant Files
- `README.md` — CLI list (~88-103) and Notes block (~105-112) where `--strict` and the contract are documented.
- `.github/workflows/release.yml` — Release CI where the dogfood `--doctor --strict` step is added.

### Dependent Files
- None.

### Related ADRs
- [ADR-001: V1 scope and exit-code contract for scriptable config validation](adrs/adr-001.md) — The contract wording and reserved-not-emitted constraint.
- [ADR-002: Atomic V1 delivery for config validation UX](adrs/adr-002.md) — Docs + dogfood are part of the single V1 release.

## Deliverables
- README CLI list + Notes documenting `--strict` and the exit-code contract.
- A copy-paste CI example running `atelier --doctor --strict`.
- A dogfood `--doctor --strict` step in `release.yml`.
- Verification that the docs link-check and workflow YAML pass **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] N/A — documentation and CI configuration only; no unit-testable crate code.
- Integration tests:
  - [x] The repository docs link-check passes for the edited README.
  - [x] The release workflow parses as valid YAML and its `--doctor --strict` step runs and exits 0 against the repo's own config.
- Test coverage target: >=80% (N/A for this docs/CI task — no source code changes; verification is the link-check and workflow run).
- All checks must pass

## Success Criteria
- All tests passing
- Test coverage >=80% (N/A — documentation/CI only)
- `--strict` and the exit-code contract are documented with no claim of an emitted code `2`.
- The release pipeline dogfoods `--doctor --strict` and the docs link-check is green.
