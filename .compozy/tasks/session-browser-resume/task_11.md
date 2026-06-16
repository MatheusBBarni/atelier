---
status: pending
title: "Resume flow (re-adopt, write lifecycle events, re-render, Idle)"
type: backend
complexity: high
dependencies:
  - task_04
  - task_05
  - task_08
  - task_10
---

# Task 11: Resume flow (re-adopt, write lifecycle events, re-render, Idle)

## Overview
Wire the end-to-end Resume action: the browser's Resume command dispatches `AppEvent::ResumeSession(session_id)`; the worker reads that session off-thread, calls `adopt_session`, appends the `run_interrupted` (if dangling) and `session_resumed` boundary events, re-renders the full prior transcript, and lands Idle so the user can submit a new prompt that appends to the same log. This delivers the crash-recovery anchor.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `AppEvent::ResumeSession(session_id)` and a browser `Resume` command (from list or preview) that dispatches it; the heavy session read MUST happen off-thread before `adopt_session` runs on the worker.
- MUST append a `session_resumed` boundary event carrying the resume context (resumed_at, cwd, head_sha, dirty, prior_end_state, approval_mode, prior_tail_hash) per ADR-002/007; and a `run_interrupted` event when the prior run was dangling (via task_10).
- MUST re-render the FULL prior transcript in the live view on resume and land in `Idle` awaiting a new prompt (no auto re-execution of the interrupted step).
- MUST ensure a new prompt submitted after resume appends to the SAME session log, and MUST honor the one-active-run guard (resume disallowed while a run is active).
</requirements>

## Subtasks
- [ ] 11.1 Add `AppEvent::ResumeSession` + the browser Resume command/dispatch.
- [ ] 11.2 Read the chosen session off-thread, then `adopt_session` on the worker.
- [ ] 11.3 Append the `session_resumed` boundary event (and `run_interrupted` if dangling).
- [ ] 11.4 Re-render the full transcript and land Idle; close the browser modal.
- [ ] 11.5 Add end-to-end resume tests (same-log append, transcript fidelity, guard).

## Implementation Details
Add the event in `src/app/mod.rs` `AppEvent` (`:289`) and handle it near `submit_prompt_with_source` (`:1018`)/the worker command loop (`src/tui/mod.rs:957`). Off-thread read mirrors `spawn_file_index_refresh` (`:947`); the result feeds `LoadedSession` → `adopt_session` (task_10). Emit events via `record_event` (`:4164`) using the kinds/payloads from task_04 and the drift/HEAD inputs from task_05. Respect the one-active-run guard (`:1054`). The Resume command originates in the browser (task_08). See TechSpec "Development Sequencing" step 9 and ADR-002.

### Relevant Files
- `src/app/mod.rs` — `AppEvent` (`:289`), `submit_prompt_with_source` (`:1018`), `record_event` (`:4164`), one-active-run guard (`:1054`).
- `src/tui/mod.rs` — worker loop (`:957`), browser Resume command (task_08), off-thread read pattern (`:947`).
- `src/app/chat/projection.rs` — fold of the re-rendered transcript (task_04 handlers).

### Dependent Files
- `src/app/mod.rs` — task_12 adds the cautious-default approval + drift interlock that resume sets up.
- `src/tui/mod.rs` — task_13 derives resume-rate metrics from the `session_resumed` events emitted here.

### Related ADRs
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — same-log append + `run_interrupted`/`session_resumed`.
- [ADR-006: Session adoption via a single adopt_session() swap + exhaustiveness test](adrs/adr-006.md) — adoption mechanism.
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — Phase 2 anchor.

## Deliverables
- `AppEvent::ResumeSession` + browser Resume action + off-thread read → adopt.
- `session_resumed` (+ conditional `run_interrupted`) emitted on resume.
- Full-transcript re-render landing Idle; new prompts append to the same log.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: full resume cycle via FakeRuntime **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Resuming appends a `session_resumed` event whose payload includes resumed_at, cwd, and head_sha.
  - [ ] Resuming a dangling session also appends a `run_interrupted` event and lands `Idle`; resuming a cleanly-terminal session appends only `session_resumed`.
  - [ ] After resume, `chat_items` contains the full prior transcript (matches a fold of the on-disk log).
  - [ ] `ResumeSession` is rejected (guard) when a run is active.
- Integration tests:
  - [ ] FakeRuntime E2E: run a prompt that ends mid-flight → open browser → Resume → both lifecycle events appended to the SAME `events.jsonl` → submit a new prompt → it appends to the same log and drives a new run.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can resume a crashed session, see the full prior thread, and continue it in the same durable log; the one-active-run invariant holds.
