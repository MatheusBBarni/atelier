---
status: pending
title: Notifier backends (OSC-native + fallback command)
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 3: Notifier backends (OSC-native + fallback command)

## Overview
Implement the built-in notifier behind an injectable `Notifier` trait: a default `OscNotifier` that emits terminal escape sequences (so notifications render locally even over SSH) and a `CommandNotifier` fallback that spawns a user-configured notifier command. This is the one V1 "battery" (ADR-002) and the primary persona's headline experience.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define a `Notifier` trait (`notify(&self, title, body) -> Result<()>`) per TechSpec "Core Interfaces".
- MUST implement `OscNotifier` as the default, emitting an OSC 9 (and the OSC 777 variant where applicable) sequence to the controlling terminal — no external binary, working over SSH.
- MUST implement `CommandNotifier` that runs `notify_fallback_command` with the title/body, using the existing subprocess idiom.
- MUST derive notification title/body from the normalized `HookPayload` fields (task_01).
- MUST keep the backend injectable so unit tests assert the emitted sequence / constructed command without real delivery.
- SHOULD document (in code) the tmux passthrough requirement for OSC.
</requirements>

## Subtasks
- [ ] 3.1 Define the `Notifier` trait in `src/hooks/notify.rs`.
- [ ] 3.2 Implement `OscNotifier` emitting OSC 9/777 to the controlling TTY.
- [ ] 3.3 Implement `CommandNotifier` spawning `notify_fallback_command`.
- [ ] 3.4 Map `HookPayload` → notification title/body with sane defaults.
- [ ] 3.5 Add unit tests asserting emitted bytes and command construction via the trait.

## Implementation Details
Create `src/hooks/notify.rs` under the task_01 module. `OscNotifier` writes the escape sequence to the controlling terminal (stderr/TTY); the exact OSC-9-vs-777 selection is internal. `CommandNotifier` reuses the `run_git`-style subprocess pattern (`src/app/git.rs:72`, `kill_on_drop` + timeout). The notifier is invoked by the dispatcher (task_04) when a handler's action is `Notify`. See TechSpec "Core Interfaces" (Notifier) and ADR-005.

### Relevant Files
- `src/hooks/notify.rs` — `Notifier` trait + `OscNotifier` + `CommandNotifier` (create).
- `src/hooks/mod.rs` — re-export the notifier types (task_01 module).
- `src/app/git.rs:72` — subprocess idiom reused by `CommandNotifier`.

### Dependent Files
- `src/hooks/dispatch.rs` — task_04 invokes the notifier for `Notify` actions.
- `src/config/mod.rs` — `notify_fallback_command` comes from `HooksConfig` (task_02).

### Related ADRs
- [ADR-005: Built-in notifier — OSC-native by default, with a configurable fallback command](../adrs/adr-005.md) — the delivery strategy and rejected alternatives.
- [ADR-002: Thin dispatcher plus one built-in battery](../adrs/adr-002.md) — why the notifier is the V1 battery.

## Deliverables
- `Notifier` trait + `OscNotifier` (default) + `CommandNotifier` (fallback).
- Title/body derivation from `HookPayload`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration coverage: the notify path is exercised through task_04's dispatcher test (cross-referenced) **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `OscNotifier::notify("Run done","ok")` writes the exact OSC 9 byte sequence (`\x1b]9;…\x07`) to its sink.
  - [ ] `CommandNotifier` builds the expected argv from `notify_fallback_command` + title/body without executing real delivery (injected runner).
  - [ ] Title/body are derived from a `run_completed` `HookPayload` with sensible defaults.
  - [ ] A `CommandNotifier` whose command exits non-zero surfaces an error rather than panicking.
- Integration tests:
  - [ ] The dispatcher (task_04) invokes the injected `Notifier` for a `notify = true` handler (cross-referenced).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `OscNotifier` emits a valid OSC sequence with no external dependency
- `CommandNotifier` runs the fallback command via the bounded subprocess idiom
- The backend is injectable and unit-testable without real OS delivery
