---
status: completed
title: Hooks core types & normalize() + public-event vocabulary
type: backend
complexity: medium
dependencies: []
---

# Task 1: Hooks core types & normalize() + public-event vocabulary

## Overview
Establish the new `src/hooks/` module with the normalized payload contract, the public-event vocabulary that maps internal event kinds to stable public names, and the pure `normalize()` projection. This is the contract foundation every other hooks task depends on; getting the payload and vocabulary right here prevents the "schema capture" the council warned about.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define the types in TechSpec "Core Interfaces" / "Data Models": `HookPayload`, `Actor`, `HooksConfig`, `HookHandler`, `HookAction`, `PayloadDetail`, `NotifyConfig`.
- MUST define a public-event vocabulary mapping stable public names to internal `HistoryEvent` kinds per ADR-004 (e.g. `step_started`→`agent_step_started`, `approval_required`→`approval_requested`, `file_edited`→`file_edit_applied`).
- MUST implement `normalize(event: &HistoryEvent, actor: ActorCtx) -> Option<HookPayload>` as a PURE function (no `App`, no IO); it MUST return `None` for any kind outside the vocabulary, including `hook_started`, `hook_completed`, and `runtime_stream_delta`.
- MUST default `PayloadDetail` to metadata-only; full payload MUST be opt-in.
- MUST define the hook-lifecycle event payload type used to record `hook_started`/`hook_completed` (consumed by task_04 emitter and task_06 projection).
- MUST register the module via `mod hooks;` in `src/lib.rs`.
- SHOULD carry `schema_version` on `HookPayload` for forward compatibility.
</requirements>

## Subtasks
- [x] 1.1 Create the `src/hooks/` module and register `mod hooks;` in `src/lib.rs`.
- [x] 1.2 Define `HookPayload` + `Actor` and the config/action types (`HooksConfig`, `HookHandler`, `HookAction`, `PayloadDetail`, `NotifyConfig`).
- [x] 1.3 Define the public-event vocabulary and the public-name↔internal-kind mapping.
- [x] 1.4 Implement `normalize()` with metadata/full handling and `None` for non-public kinds.
- [x] 1.5 Define the hook-lifecycle event payload type for `hook_started`/`hook_completed`.
- [x] 1.6 Add unit tests for the mapping and `normalize()`.

## Implementation Details
Create `src/hooks/mod.rs` (optionally split payload types into `src/hooks/payload.rs`). Consume `history::HistoryEvent` (`src/history/mod.rs:12`). `ActorCtx` carries `{ agent: Option<String>, runtime: Option<String> }`; the live tap (task_05) populates it from `active_step.agent` + `AgentProfile.runtime`, and the follow reader (task_07) reconstructs it. Mirror existing serde derive conventions. Do NOT implement config-merge (task_02), dispatch (task_04), or notifier (task_03) here. See TechSpec "Core Interfaces" and "Data Models".

### Relevant Files
- `src/hooks/mod.rs` — new module home for all hooks types and `normalize()` (create).
- `src/lib.rs` — register `mod hooks;`.
- `src/history/mod.rs:12` — `HistoryEvent` shape consumed by `normalize()`.
- `src/config/mod.rs:251,315` — `RuntimeKind` / `AgentProfile.runtime`, the source of the uniform `actor` field.

### Dependent Files
- `src/config/mod.rs` — task_02 embeds `HooksConfig` and threads it through the ladder.
- `src/app/mod.rs` — task_05 tap calls `normalize()`.
- `src/app/chat/projection.rs` — task_06 reads the hook-lifecycle payload type.
- `src/cli.rs` / follow reader — task_07 calls `normalize()`.

### Related ADRs
- [ADR-004: Handler-array config schema and a normalized, versioned cross-runtime payload contract](../adrs/adr-004.md) — defines the payload, vocabulary, and `normalize()`.
- [ADR-001: V1 ships cross-runtime observer hooks with a decision-first payload contract](../adrs/adr-001.md) — curated-subset rationale.

## Deliverables
- `src/hooks/` module with all payload/config/action types and the public-event vocabulary.
- A pure `normalize(event, actor) -> Option<HookPayload>` function.
- The hook-lifecycle event payload type for `hook_started`/`hook_completed`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration coverage of `normalize()` is exercised end-to-end by task_05's fake-runtime test (cross-referenced) **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `normalize()` maps internal `agent_step_started` → public `step_started` with a populated `actor { agent, runtime }`. — `maps_agent_step_started_to_public_step_started_with_actor`
  - [x] `normalize()` returns `None` for `runtime_stream_delta` and for `hook_completed` (non-public kinds never re-trigger hooks). — `returns_none_for_non_public_kinds`
  - [x] `PayloadDetail::Metadata` (default) omits body fields; `PayloadDetail::Full` includes them. — `metadata_omits_body_and_full_includes_it`
  - [x] Every ADR-004 public name resolves to an internal kind; an unknown public name is absent from the map. — `every_public_name_round_trips_and_unknowns_are_absent`
  - [x] `actor` is `None`/orchestrator for a pre-agent event (e.g. `run_started`). — `actor_is_orchestrator_for_a_pre_agent_event`
- Integration tests:
  - [ ] `normalize()` output shape asserted through task_05's end-to-end fake-runtime run (cross-referenced; deferred to task_05).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `src/hooks/` compiles and is registered in `src/lib.rs`
- `normalize()` is pure (no `App`/IO dependencies) and returns `None` outside the public vocabulary
- The public vocabulary matches the ADR-004 mapping table

## As-built notes
- **`normalize()` takes a third `detail: PayloadDetail` argument** — `normalize(event, actor, detail) -> Option<HookPayload>`, refining the two-argument sketch in the techspec "Core Interfaces". The detail must be an input so the *single* shared projection serves both the per-handler live tap (task_05, which knows its handler's `PayloadDetail`) and the `--events follow` reader (task_07), and so the metadata-vs-full contract is realized inside one pure function. Downstream callers pass `handler.payload` (tap) or a chosen detail (follow).
- **`HookPayload` carries an optional `body: Option<serde_json::Value>`** (`skip_serializing_if = "Option::is_none"`): `None`/omitted under `PayloadDetail::Metadata` (the default), the raw event payload under `Full`. Redaction is applied downstream by the dispatcher (task_04) / follow reader (task_07) — `normalize()` stays pure.
- **Extras for downstream tasks:** `ActorCtx` (input type), `HookLifecyclePayload` (Serialize+Deserialize, for task_04 emit / task_06 projection), the `public_name_for_kind` / `internal_kind_for_public` vocabulary accessors, and `HookHandler::matches` / `HookAction::kind_str` helpers.
- **`target`/`outcome` extraction** in `normalize()` is best-effort and tolerant of absent fields: `target` from `action_requested` (`params.command|path`) and `file_edit_applied` (`path` or first `changed_files`); `outcome` for the terminal kinds + `file_edit_applied`. No task_01 acceptance test pins specific target/outcome values, but coverage tests assert the common cases.
