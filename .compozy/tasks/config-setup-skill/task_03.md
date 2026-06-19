---
status: completed
title: references/presets.md — named starter presets
type: docs
complexity: medium
dependencies: []
---

# references/presets.md — named starter presets

## Overview
Author `references/presets.md`: a set of named, self-contained starter configs (each a single ` ```toml ` block) the wizard offers based on the detected runtime CLI. Each preset must load standalone under the real config loader — using inline agent `instructions` (no external prompt-file dependencies) and `api_key_env` names (never secrets) — so a user can adopt one in under a minute and pass `atelier --print-config` on the first write.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST provide named presets as self-contained ` ```toml ` blocks: **Claude-only**, **Codex + Claude fallback**, **Cursor**, and **Z.ai HTTP** (F2 / techspec presets list).
- MUST make each preset load standalone via the config loader: set `schema_version = 1`, an `approval_mode`, the needed `[runtimes.*]`, and at least an `[agents.orchestrator]` with **inline `instructions`** (no `instructions_file` / external-file deps).
- MUST set credentials as `api_key_env` (env-var name, e.g. `ZAI_API_KEY`) only — never inline a secret value (Z.ai preset especially).
- MUST keep model strings free-form/plausible (they load regardless) and instruct the user to confirm the real current model rather than asserting a specific model.
- MUST ensure presets only use real fields/enum values from `references/config-schema.md` (task_02) so they pass the TOML-load + unknown-field gate.
- SHOULD note when a preset references an optional capability/section (e.g. MCP) that requires extra setup, without making a preset depend on an unavailable server.
</requirements>

## Subtasks
- [x] 3.1 Author the **Claude-only** preset (claude runtime + orchestrator with inline instructions).
- [x] 3.2 Author the **Codex + Claude fallback** preset (codex primary, model_fallbacks to a claude model).
- [x] 3.3 Author the **Cursor** preset.
- [x] 3.4 Author the **Z.ai HTTP** preset (`base_url` + `api_key_env = "ZAI_API_KEY"`, no secret value).
- [x] 3.5 Add a short per-preset note on what to confirm (model name, exported env var) and how to validate.

## Implementation Details
Create `skills/atelier-config-setup/references/presets.md`. Base each block on the loader's expectations and the `--init-config` starter (`starter_config_text()` in `src/config/mod.rs`) but make each preset minimal and standalone (inline instructions so there are no `agents/*.md` file dependencies). The Zai runtime requires `api_key_env`; codex/claude/cursor must NOT set `api_key_env` (the loader rejects it for those kinds). See TechSpec "Data Models (references/presets.md)" and PRD F2.

### Relevant Files
- `src/config/mod.rs` — `into_effective` per-runtime validation (codex/claude/cursor reject `api_key_env`; zai requires it; ~1731+), `RuntimeKind`, `AgentProfile` required fields (runtime, model, capabilities, instructions), `starter_config_text()`.
- `skills/atelier-config-setup/references/config-schema.md` (task_02) — the field/enum source presets must conform to.

### Dependent Files
- `tests/atelier_config_skill.rs` (task_06) — TOML-load test parses every preset block.
- `skills/atelier-config-setup/SKILL.md` (task_01) — the wizard offers these presets.

### Related ADRs
- [ADR-001: Essentials-first wizard with named presets](../adrs/adr-001.md)
- [ADR-005: Every shipped preset must load under the config loader](../adrs/adr-005.md)
- [ADR-003: Greenfield — presets build fresh config, no import](../adrs/adr-003.md)

## Deliverables
- `skills/atelier-config-setup/references/presets.md` with 4 named, standalone-loadable presets, secrets-by-env-name only.
- Each preset block parses via the loader (asserted in task_06).
- Tests asserting preset load + shape **(REQUIRED)** — implemented in task_06.

## Tests
(Asserted by the task_06 module.)
- Unit tests:
  - [ ] Each preset block sets a runtime and an `[agents.orchestrator]` (preset-shape smoke). (task_06)
  - [ ] No preset inlines a secret value; Z.ai uses `api_key_env`. (task_06 — assert no `sk-`/`zai-`-looking literals; `api_key_env` present)
- Integration tests:
  - [ ] Every preset ` ```toml ` block loads via `load_effective_config`/`RawConfig` standalone (no external file deps, no unknown keys). (task_06 TOML-load)
- Test coverage target: >=80% (of the preset blocks)
- All tests must pass

## Success Criteria
- All tests passing (via task_06)
- All 4 presets load standalone and cleanly; ≥50%-preset-adoption path (PRD metric) is enabled.
- No secret values appear in any preset; credentials are env-var names.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
