---
status: pending
title: "Resume-rate instrumentation + dynamic post-crash hint"
type: backend
complexity: medium
dependencies:
  - task_03
  - task_09
  - task_11
---

# Task 13: Resume-rate instrumentation + dynamic post-crash hint

## Overview
Close the loop the PRD/Devil's-Advocate flagged: make the success metrics computable from the durable log, and replace the static welcome cue with a dynamic post-crash hint when the most recent session ended non-terminally. This turns the "is resume frequent enough?" unknown into data and nudges users to recover.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST ensure the PRD success metrics are derivable by folding the log: crash-recovery adoption (non-terminal-ending sessions later getting a `session_resumed`), time-to-continue (resume → first new `prompt_submitted`), and resumed-session completion (resumed sessions reaching terminal `Completed`). Add any missing structured fields needed (no external telemetry).
- MUST show a dynamic hint in the welcome facts box when the newest session's outcome is non-terminal (interrupted/dangling), telling the user it can be reopened (via the browser / `Ctrl-R`).
- MUST suppress the dynamic hint when the newest session ended cleanly (avoid false "crash" positives on a normal quit).
- MUST compute the newest-session outcome from `list_session_summaries` (task_03), not by ad-hoc scanning.
</requirements>

## Subtasks
- [ ] 13.1 Confirm/add the structured fields needed to derive the three metrics from the log.
- [ ] 13.2 Add a metrics derivation helper (folds the log; no external sink).
- [ ] 13.3 Wire the dynamic post-crash hint into the welcome facts box (newest non-terminal session only).
- [ ] 13.4 Add tests for metric derivation and hint show/suppress logic.

## Implementation Details
Derive metrics from the events emitted in task_11 (`session_resumed`) + run outcomes; add a small helper (in `src/history/mod.rs` or `src/app/mod.rs`) over `list_session_summaries` (task_03) and the log. Extend the welcome cue from task_09 in `src/tui/welcome.rs` (`WelcomeFacts` `:92`, facts `:312`) to a dynamic variant gated on the newest summary's `outcome` being non-terminal (`RunState::is_terminal()` == false). See PRD "Success Metrics"/"Monitoring and Observability" and ADR-005.

### Relevant Files
- `src/history/mod.rs` — `list_session_summaries` (task_03); metrics derivation helper.
- `src/tui/welcome.rs` — `WelcomeFacts` (`:92`), facts (`:312`); static cue from task_09.
- `src/app/mod.rs` — startup wiring of the newest-session outcome into the welcome facts.

### Dependent Files
- None downstream (final task of the feature).

### Related ADRs
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — instrument resume-rate; phase gating depends on this data.
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — metrics derive from the lifecycle events in the log.

## Deliverables
- Log-derivable computation of the three resume metrics (no external telemetry).
- Dynamic post-crash hint in the welcome facts box (shown only for a non-terminal newest session).
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: a crashed-then-resumed fixture yields the expected metric values **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Crash-recovery adoption: a fixture with N non-terminal-ending sessions, M of which later have a `session_resumed`, derives the ratio M/N.
  - [ ] Time-to-continue: derived as the delta between a `session_resumed` and the next `prompt_submitted` in that session.
  - [ ] The welcome hint is shown when the newest session's outcome is non-terminal and suppressed when it ended `Completed`.
  - [ ] The newest-session outcome comes from `list_session_summaries` (no ad-hoc scan).
- Integration tests:
  - [ ] FakeRuntime E2E: a session ends mid-flight and is resumed and completed; the derived metrics reflect one recovered + completed resume, and a fresh launch shows the post-crash hint until resumed.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The three PRD success metrics are computable from the log alone, and users are proactively nudged to recover a crashed session.
