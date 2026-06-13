---
status: pending
title: "Submission provenance tagging at submit"
type: backend
complexity: medium
dependencies:
  - task_03
  - task_05
---

# Submission provenance tagging at submit

## Overview

Close the KPI loop: tag each submission `Recalled` when it originated from the recall
ring (cursor ≠ 0) and write that into the `prompt_submitted` payload, and keep the
in-session ring current by prepending each submitted prompt (ADR-003, ADR-004). This
makes "recall adoption" measurable directly from the event log.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST set the submit event's `PromptSource` to `Recalled` iff `prompt_history_cursor != 0` at submit time, else `Fresh`, in the TUI submit path (where `ui_state` is available).
- MUST thread the source through `App::submit_prompt` and add a `"source": "fresh"|"recalled"` field to the `prompt_submitted` payload.
- MUST prepend the just-submitted prompt to `prompt_history` (consecutive-dedup, respect `prompt_history_max`) and reset `prompt_history_cursor` to 0 on submit.
- MUST default a missing `source` (older events) to `fresh` on read.
- MUST NOT alter any other event payload.
</requirements>

## Subtasks
- [ ] 6.1 Determine `source` from cursor state in the TUI `Dispatch(PromptSubmitted)` handler.
- [ ] 6.2 Thread `source` through `submit_prompt` into `record_event`.
- [ ] 6.3 Add the `"source"` field to the `prompt_submitted` payload.
- [ ] 6.4 Prepend the submitted prompt to the ring (dedup + cap) and reset the cursor.
- [ ] 6.5 Test the tagging and the in-session prepend.

## Implementation Details

Edit `src/tui/mod.rs` (the `TuiCommand::Dispatch(AppEvent::PromptSubmitted)` arm in
`execute_tui_command`, ~`:565`, plus `clear_input`) and `src/app/mod.rs`
(`submit_prompt` `:911`, the `prompt_submitted` `json!` payload ~`:976`, and the
`PromptSubmitted` worker handler `:837`). The source is finalized in the TUI submit
handler because that is where `ui_state.prompt_history_cursor` is visible. See TechSpec
"Implementation Design → Data Models" (payload) and "System Architecture" (data flow:
submit + tag).

### Relevant Files
- `src/tui/mod.rs` — the submit handler (cursor→source, ring prepend, cursor reset) and `clear_input`.
- `src/app/mod.rs` — `submit_prompt`, `record_event`, the `prompt_submitted` payload, and the `PromptSubmitted` handler.

### Dependent Files
- (none downstream; this completes the recall data path.)

### Related ADRs
- [ADR-003: Recall State in TuiUiState; Tag Submissions via Extended AppEvent](../adrs/adr-003.md) — provenance rule (`cursor != 0` → `Recalled`).
- [ADR-004: Asynchronous Background History Projection](../adrs/adr-004.md) — in-session prepend keeps recall current without a disk reload.

## Deliverables
- Source tagging at submit, a `"source"` payload field, and the in-session ring prepend.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: a recalled submit writes `source: "recalled"` to the event log **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Submit with `prompt_history_cursor == 2` → event source is `Recalled`.
  - [ ] Submit freshly typed text (`cursor == 0`) → source is `Fresh`.
  - [ ] Recall → backspace to empty → type new text → submit → `Fresh` (cursor reset on clear).
  - [ ] After submitting "gamma", `prompt_history[0] == "gamma"` (prepended; deduped if equal to the prior front; cap respected) and `prompt_history_cursor == 0`.
- Integration tests:
  - [ ] Via the `fake` runtime: submit a recalled prompt → the persisted `prompt_submitted` payload contains `"source":"recalled"`.
  - [ ] Reading an event that lacks `source` → treated as `fresh`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Recalled vs fresh submissions are correctly tagged in the event log
- The in-session prepend works and the payload change is backward-compatible
