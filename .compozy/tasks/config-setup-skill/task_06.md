---
status: completed
title: Rust drift/correctness guard (tests/atelier_config_skill.rs)
type: test
complexity: high
dependencies:
  - task_01
  - task_02
  - task_03
  - task_04
  - task_05
---

# Rust drift/correctness guard (tests/atelier_config_skill.rs)

## Overview
Implement the deterministic, no-LLM test module that proves the shipped skill is correct and in sync with the code: it is discoverable with valid frontmatter, its schema doc covers every enum variant (and only real ones), every fenced ` ```toml ` block loads under the real config loader, and the discovery mirrors equal the canonical source. This is the CI guarantee behind the PRD's "0 hallucinated keys / ≥90% validity" targets (ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `tests/atelier_config_skill.rs` with four test areas: (1) discoverability, (2) enum-coverage drift, (3) TOML-block load, (4) mirror equality.
- MUST (discoverability) load `atelier-config-setup` through the real skills-discovery module from a discovery root with valid `{name, description}` frontmatter, reusing existing test patterns.
- MUST (enum-coverage) assert every serde variant of `RuntimeKind`/`ApprovalMode`/`AgentEffort`/`Capability`/`ToolName` (via each enum's `all()` from task_04) appears in `references/config-schema.md` by serde name, with **no stray variant strings**, and that the documented `schema_version == 1` — this MUST catch a missing `mcp_tool` / MCP `ToolName`.
- MUST (TOML-load) extract every ` ```toml ` fenced block from `SKILL.md` + `references/config-schema.md` + `references/presets.md` and assert each loads via the config loader (`RawConfig` parse / `load_effective_config`), catching unknown keys and bad enum values.
- MUST (mirror equality) assert each generated discovery mirror (per task_05's chosen approach) is byte-equal to the canonical `skills/atelier-config-setup/` tree.
- MUST resolve paths relative to `CARGO_MANIFEST_DIR` (the canonical/mirror files are repo paths, not test fixtures).
- SHOULD keep any LLM/eval test out of CI (env-gated `#[ignore]` like `tests/runtime_integration.rs`) — V1 ships only the deterministic guards.
</requirements>

## Subtasks
- [x] 6.1 Discoverability test: skills module lists `atelier-config-setup` with valid frontmatter.
- [x] 6.2 Enum-coverage drift test: every `all()` variant documented, no strays, `schema_version == 1`.
- [x] 6.3 TOML-extraction helper + load test over `SKILL.md` + both references.
- [x] 6.4 Mirror-equality test against the canonical tree.
- [x] 6.5 Preset-shape smoke + secret-absence assertions for the preset blocks.

## Implementation Note
Variant serde names are derived from each enum's own `Serialize` impl (via `serde_json::to_value`)
so the test can never disagree with the loader. Enum coverage uses set-equality between the union of
all five `all()` serde names and the tokens inside `config-schema.md`'s dedicated ` ```text ` blocks —
catching both missing variants (e.g. `mcp_tool`) and strays. TOML blocks load via
`load_effective_config` with an explicit `config_path` + empty temp `working_directory`, isolating the
real home/local layers. Mirror equality follows symlinks (content-equality), matching task_05's
contract.

## Implementation Details
Create `tests/atelier_config_skill.rs`. Reuse the existing config loader (`multiagent::config::{load_effective_config, ConfigLoadOptions}` / `RawConfig`) and the skills-discovery module (`src/skills/mod.rs`) — mirror the patterns in `tests/skills_foundation.rs` and `tests/skill_prompt_loading.rs`. Use the `all()` functions added in task_04 to enumerate variants, comparing serde names (snake_case; `XHigh => "xhigh"`). Read canonical/mirror files via `env!("CARGO_MANIFEST_DIR")` joins. See TechSpec "Implementation Design → Test entry points" / "Testing Approach" and ADR-005.

### Relevant Files
- `tests/skills_foundation.rs`, `tests/skill_prompt_loading.rs` — discovery test patterns to mirror.
- `src/skills/mod.rs` — discovery entry points, `SkillManifest`, `skill_roots()`.
- `src/config/mod.rs` — `load_effective_config`, `RawConfig`, the 5 enums + their `all()` (task_04), `schema_version`.
- `skills/atelier-config-setup/SKILL.md` + `references/*.md` (task_01/02/03) — the content under test.
- `.agents/skills/atelier-config-setup/`, `.claude/skills/atelier-config-setup/` (task_05) — mirrors to compare.

### Dependent Files
- `.github/workflows/release.yml` (task_09) — runs this test suite in CI.

### Related ADRs
- [ADR-005: Skill correctness via lightweight Rust tests + enum/TOML drift guard](../adrs/adr-005.md)
- [ADR-004: Mirror-equality test (shared)](../adrs/adr-004.md)

## Deliverables
- `tests/atelier_config_skill.rs` implementing discoverability + enum-coverage + TOML-load + mirror-equality (+ preset smoke), all passing against the task_01/02/03/04/05 outputs.
- Unit + integration tests **(REQUIRED)** with ≥80% coverage of the asserted skill surface.

## Tests
- Unit tests:
  - [ ] `schema_doc_covers_all_enum_variants_and_version`: each enum's `all()` serde name is present in `config-schema.md`; no stray variant; `schema_version == 1`.
  - [ ] Preset-shape smoke: each preset block declares a runtime + an orchestrator agent; no inlined secret literal.
- Integration tests:
  - [ ] `skill_is_discoverable_with_valid_frontmatter`: discovery lists `atelier-config-setup` from a root with parseable `name`/`description`.
  - [ ] `every_toml_block_loads_under_the_config_loader`: every fenced `toml` block from SKILL.md + references + presets parses via the loader (no unknown keys / bad enums).
  - [ ] `mirrors_equal_canonical_source`: each generated mirror is byte-equal to `skills/atelier-config-setup/`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- The drift guard fails loudly on an undocumented enum variant (e.g. a future config addition), an unknown key in any block, or a drifted mirror.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
