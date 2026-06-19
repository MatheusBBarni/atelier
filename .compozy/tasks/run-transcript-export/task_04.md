---
status: pending
title: "Export orchestrator and risk-adaptive review gate"
type: backend
complexity: medium
dependencies:
  - task_01
  - task_02
  - task_03
---

# Task 04: Export orchestrator and risk-adaptive review gate

## Overview
Add `src/export.rs` with `export_session(cfg, opts)` and the `ExportOptions`/`ExportOutcome`/`SessionSelector` types — the orchestrator that resolves the target session/run, builds the preview, renders Markdown, scans for secrets, applies the risk-adaptive review gate, writes the `0600` quarantined file, and appends the `session_exported` audit event. This is the feature's spine, tying tasks 01–03 together.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `pub mod export;` (`src/export.rs`) exposing `ExportOptions`, `ExportOutcome`, `SessionSelector`, and `pub fn export_session(cfg: &EffectiveConfig, opts: &ExportOptions) -> Result<ExportOutcome>` per TechSpec "Core Interfaces".
- MUST resolve `SessionSelector::Latest` via `list_session_summaries(root).first()` and `Explicit(id)` via `HistoryStore::open`; scope to a single run when `opts.run` is set.
- MUST reuse `build_session_preview` for the read path — no new log reading or projection.
- MUST implement the risk-adaptive gate: interactive clean → `y`/`yes`; interactive flagged → `approve`; `--yes` clean → write; `--yes` flagged → fail closed (`bail!`) unless `allow_flagged`.
- MUST write the transcript via the `pub(crate)` `write_private_file` (`0600`) to the default `.atelier/exports/<id>-<ts>.md` or `opts.out` (`-` = stdout), and append a `session_exported` event recording scope/counts/override.
- Gate prompts and warnings MUST go to stderr; the path/transcript to stdout. The gate DECISION MUST be a pure, unit-testable function separate from IO.
- MUST NOT instantiate `App`; append via `HistoryStore::open(...).append_event(...)`.
</requirements>

## Subtasks
- [ ] 4.1 Define `ExportOptions`/`ExportOutcome`/`SessionSelector` and the module.
- [ ] 4.2 Resolve the session (latest/explicit) and run-scope, then call `build_session_preview`.
- [ ] 4.3 Render Markdown and run `scan_secrets`; compute deterministic/advisory counts.
- [ ] 4.4 Implement the pure gate-decision function and the interactive / `--yes` flows.
- [ ] 4.5 Write the quarantined `0600` file (or stdout) and append the `session_exported` event.

## Implementation Details
Create `src/export.rs` and add `pub mod export;` to `src/lib.rs`. Compose task_01 (kind + writer), task_02 (`scan_secrets`), and task_03 (`render_session_markdown`). See TechSpec "Core Interfaces", "Data Models" (event payload), and "Development Sequencing". The egress-safety warning is layered on in task_05; the CLI dispatch arm is task_06.

### Relevant Files
- `src/history/mod.rs` — `HistoryStore::open`/`read_events`/`append_event`, `list_session_summaries`, `HistoryEvent::new`, `SESSION_EXPORTED_KIND` + `write_private_file` (task_01).
- `src/app/chat/mod.rs` — `build_session_preview`, `SessionPreview`.
- `src/app/chat/markdown.rs` — `render_session_markdown` (task_03).
- `src/runtime/status.rs` — `scan_secrets` (task_02).
- `src/config/mod.rs` — `EffectiveConfig.working_directory`.
- `src/cli.rs` — `confirm_cleanup` (~235) as the stdin/stderr prompt pattern to mirror.

### Dependent Files
- `src/lib.rs` — add `pub mod export;`.
- `src/cli.rs` (task_06) — dispatch arm builds `ExportOptions` and calls `export_session`.
- `src/export.rs` (task_05) — egress warning wired into the write step.

### Related ADRs
- [ADR-003: Component architecture — dedicated export module](../adrs/adr-003.md) — orchestration home and reuse of `build_session_preview`.
- [ADR-002: Risk-adaptive review gate](../adrs/adr-002.md) — `y`/`approve`/`--yes` semantics.
- [ADR-004: Fail-closed enforcement](../adrs/adr-004.md) — `bail!` on Deterministic hits under `--yes`.

## Deliverables
- `src/export.rs` with the option/outcome types, `export_session`, and a pure gate-decision function.
- `pub mod export;` wiring in `src/lib.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests at binary level provided in task_06 **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Gate decision: clean + interactive `y` → Write; clean + `n` → Cancel.
  - [ ] Gate decision: flagged + interactive `approve` → Write; flagged + `y` → Reject (insufficient).
  - [ ] Gate decision: flagged + `--yes` without `allow_flagged` → FailClosed; with `allow_flagged` → Write.
  - [ ] `SessionSelector::Latest` resolves the newest session id from a seeded temp `.atelier`.
  - [ ] `opts.run = Some(id)` yields only that run's items in the rendered output.
  - [ ] On write, a `session_exported` event with correct scope/counts is appended and the file is `0600`.
- Integration tests:
  - [ ] (covered in task_06) full binary invocation produces the file + event.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `export_session` produces a redacted file + audit event; gate logic correct across all tiers
- No `App` instantiation; read path reuses `build_session_preview`
