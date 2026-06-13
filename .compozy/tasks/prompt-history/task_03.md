---
status: pending
title: "PromptSource enum and AppEvent::PromptSubmitted extension"
type: refactor
complexity: medium
dependencies: []
---

# PromptSource enum and AppEvent::PromptSubmitted extension

## Overview

Introduce a `PromptSource { Fresh, Recalled }` enum and extend the widely-referenced
`AppEvent::PromptSubmitted(String)` variant to carry it, so a submission's provenance
can flow to the worker for the recall-adoption KPI (ADR-003). This is a mechanical,
compile-checked change across every construction and match site; all existing sites
default to `Fresh`, leaving behavior unchanged.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `enum PromptSource { Fresh, Recalled }` (deriving `Copy`, `Debug`, `PartialEq`, `Eq`).
- MUST change `AppEvent::PromptSubmitted(String)` to `AppEvent::PromptSubmitted(String, PromptSource)`.
- MUST update every construction site to pass an explicit source — all existing, non-recall sites use `Fresh` — and every match site to bind or ignore the new field.
- MUST keep the workspace compiling and behavior unchanged (all `Fresh`); this task changes the signature only.
- MUST NOT yet write `source` into the event payload (that is task_06).
</requirements>

## Subtasks
- [ ] 3.1 Define `PromptSource` in the app event module.
- [ ] 3.2 Extend the `AppEvent::PromptSubmitted` variant.
- [ ] 3.3 Update all construction sites (TUI Enter handler, app/test fixtures) to `Fresh`.
- [ ] 3.4 Update all match sites (the worker handler, tests).
- [ ] 3.5 Add a unit test asserting the variant carries and exposes a source.

## Implementation Details

`AppEvent` is defined at `src/app/mod.rs:213`; `PromptSubmitted` is referenced in
`src/app/mod.rs` (~18 sites, including the worker handler at `:837` and many tests)
and `src/tui/mod.rs` (~12 sites, including the Enter→submit construction near `:1137`
and key-handling tests). See TechSpec "Implementation Design → Core Interfaces"
(provenance). Sequence this task early — it gates compilation of tasks 05 and 06.

### Relevant Files
- `src/app/mod.rs` — `AppEvent` enum (`:213`), the `PromptSubmitted` worker handler (`:837`), and test fixtures.
- `src/tui/mod.rs` — the Enter→`PromptSubmitted` construction (~`:1137`) and key-handling tests.

### Dependent Files
- `src/tui/mod.rs` (task_06) — will set `Recalled` based on cursor state.
- `src/app/mod.rs` (task_06) — `submit_prompt`/`record_event` will write `source` into the payload.

### Related ADRs
- [ADR-003: Recall State in TuiUiState; Tag Submissions via Extended AppEvent](../adrs/adr-003.md) — the decision to carry provenance on the submit event.

## Deliverables
- `PromptSource` enum and the extended variant; all sites compile with explicit sources.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration: existing app/TUI submit tests pass unchanged with `Fresh` **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Constructing `AppEvent::PromptSubmitted("x".into(), PromptSource::Fresh)` and matching it extracts both fields.
  - [ ] `PromptSource` is `Copy` and `PartialEq` (compile + `assert_eq!(Recalled, Recalled)` / `assert_ne!(Fresh, Recalled)`).
- Integration tests:
  - [ ] The Enter-submits test yields `PromptSubmitted(input, Fresh)` after the signature change.
  - [ ] The worker `handle_event(PromptSubmitted(_, Fresh))` path still drives a run end-to-end via the `fake` runtime.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The workspace compiles with explicit `PromptSource` at every site
- Behavior is unchanged (all `Fresh`); no payload change yet
