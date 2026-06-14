---
status: completed
title: "UI config: prompt-history enable flag and size cap"
type: backend
complexity: low
dependencies: []
---

# UI config: prompt-history enable flag and size cap

## Overview

Add the two configuration knobs that gate and bound recall — `prompt_history_enabled`
(default true) and `prompt_history_max` (default 200) — to the `[ui]` config section,
so the feature is on by default with a disable toggle (PRD enablement decision) and a
bounded recall list.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `prompt_history_enabled: bool` and `prompt_history_max: usize` to `UiConfig`.
- MUST add the matching `Option` fields to `RawUiConfig` and merge them in the `[ui]` merge path next to `hide_banner`.
- MUST default to `prompt_history_enabled = true` and `prompt_history_max = 200` when the keys are omitted.
- MUST preserve layered precedence (built-in default → home → local) for both keys.
- SHOULD document the two keys (doc-comment and/or a sample `[ui]` snippet) so users can discover the toggle.
</requirements>

## Subtasks
- [x] 2.1 Add the two fields to `UiConfig` with defaults.
- [x] 2.2 Add the two optional fields to `RawUiConfig`.
- [x] 2.3 Extend the `[ui]` merge to apply both.
- [x] 2.4 Document the keys briefly.
- [x] 2.5 Cover default, override, and layer-precedence behavior with tests.

## Implementation Details

Edit `src/config/mod.rs`: `UiConfig` (near `hide_banner`), `RawUiConfig`, and the
`[ui]` merge arm that currently folds `hide_banner`. See TechSpec "Implementation
Design → Core Interfaces" (config) and "Data Models". Defaults follow ADR-002
(on-by-default); the cap feeds ADR-004's bounded projection.

### Relevant Files
- `src/config/mod.rs` — `UiConfig`, `RawUiConfig`, and the layered merge for the `[ui]` section.

### Dependent Files
- `src/tui/mod.rs` — task_04 reads `prompt_history_enabled` to gate the loader and `prompt_history_max` to cap the ring.

### Related ADRs
- [ADR-002: V1 Ships the Full Faithful-Parity Recall Set in One Release](../adrs/adr-002.md) — on-by-default with a disable toggle.

## Deliverables
- Two new `[ui]` keys with defaults and merge wiring.
- Brief doc/sample for the keys.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test confirming a `multiagent.toml` `[ui]` override flows into the effective config **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Omitted keys → effective `prompt_history_enabled == true` and `prompt_history_max == 200`.
  - [ ] `[ui] prompt_history_enabled = false` → effective value is `false`.
  - [ ] `[ui] prompt_history_max = 50` → effective value is `50`.
  - [ ] Home config sets `false`, local config sets `true` → local wins (layer precedence).
- Integration tests:
  - [ ] A `multiagent.toml` with `[ui] prompt_history_max = 10` loads into the effective config with `ui.prompt_history_max == 10`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Defaults are `true` / `200`; overrides and layer precedence are honored
- The toggle is documented for discoverability
