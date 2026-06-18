---
status: completed
title: Author canonical SKILL.md wizard (scaffold + protocol)
type: docs
complexity: medium
dependencies: []
---

# Author canonical SKILL.md wizard (scaffold + protocol)

## Overview
Create the canonical, engine-agnostic `skills/atelier-config-setup/SKILL.md` — the portable wizard any LLM agent (atelier's runtime or an external agent) loads to author a valid `atelier.toml`. The body encodes the essentials-first wizard protocol, the self-validate-with-degradation protocol, the secret rule, and the anti-drift instruction, while deferring the bulk schema/preset detail to `references/` (progressive disclosure).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `skills/atelier-config-setup/SKILL.md` with frontmatter limited to `name: atelier-config-setup` and a trigger-rich third-person `description` (atelier's `SkillManifest` ignores extra keys, so optional skills.sh keys are allowed but unnecessary).
- MUST keep the body under 500 lines, pushing schema/preset detail into `references/` (F1, anti-drift body budget).
- MUST document the wizard protocol: preset selection (suggested by detected runtime CLI) → essentials (runtime → model + fallbacks → `approval_mode` → one starter agent) → optional progressive disclosure of `presets`/`council`/`limits`/`ui`/`workspace`.
- MUST document the self-validate protocol: if `atelier` is on PATH, run `atelier --print-config` (hard gate) then `atelier --doctor --json` (advisory), fix-and-retry; otherwise write a schema-correct config and instruct the user to run `atelier --doctor` (never block on atelier being installed — F4).
- MUST state the secret rule: set `api_key_env` (the env-var name) only; never write secret values into TOML.
- MUST state the anti-drift instruction: read `atelier --init-config` / `--print-config` output as ground truth before editing, and note the config `schema_version` the skill targets (F5).
- MUST point to `references/config-schema.md` and `references/presets.md` for the full schema and presets (consumed by task_02/task_03).
</requirements>

## Subtasks
- [x] 1.1 Create the `skills/atelier-config-setup/` directory and `SKILL.md` with minimal `name`+`description` frontmatter.
- [x] 1.2 Write the purpose + when-to-use sections (trigger phrases like "set up my atelier config").
- [x] 1.3 Write the essentials-first wizard protocol with preset suggestion by detected CLI.
- [x] 1.4 Write the self-validate-with-degradation protocol (`--print-config` gate, `--doctor` advisory, write-and-instruct fallback).
- [x] 1.5 Write the secret rule + anti-drift instruction + `references/` pointers; keep body < 500 lines.

## Implementation Details
Create `skills/atelier-config-setup/SKILL.md` (new top-level `skills/` root per ADR-004). Mirror the frontmatter shape atelier's discovery expects — `SkillManifest` parses `name`/`description` from the `---` YAML block and tolerates extra keys (no `deny_unknown_fields`). Model the body's tone/structure on the existing in-repo skills under `.agents/skills/*/SKILL.md`. The wizard/self-validate behaviors are *instructions* (prompt-injected), not executable code. See TechSpec "Implementation Design → Data Models (SKILL.md)" and ADR-001 (scope/anti-drift) / ADR-005 (the fenced-`toml` convention the drift test will rely on — any example config blocks in this file must be valid).

### Relevant Files
- `src/skills/mod.rs` — `SkillManifest` (~115), `parse_skill_manifest` (~361), `SKILL_FILE_NAME = "SKILL.md"`, `SKILL_DISCOVERY_MAX_DEPTH = 4`, `skill_roots()` (~328). Defines what discovery expects.
- `.agents/skills/*/SKILL.md` — existing skills to mirror in tone/structure.
- `src/cli.rs` — `--init-config` (~137) / `--print-config` (~181) the protocol instructs running.

### Dependent Files
- `skills/atelier-config-setup/references/config-schema.md` (task_02) — referenced by this SKILL.md.
- `skills/atelier-config-setup/references/presets.md` (task_03) — referenced by this SKILL.md.
- `npm/scripts/sync-skills.mjs` (task_05) — mirrors this canonical file.

### Related ADRs
- [ADR-001: Portable config-setup skill, essentials-first wizard, anti-drift posture](../adrs/adr-001.md)
- [ADR-004: Canonical skill in a top-level `skills/` directory with generated mirrors](../adrs/adr-004.md)
- [ADR-005: Skill correctness via lightweight Rust tests + an enum/TOML drift guard](../adrs/adr-005.md) — fenced-`toml` blocks must load.

## Deliverables
- `skills/atelier-config-setup/SKILL.md` with valid `name`/`description` frontmatter, body < 500 lines, covering wizard + self-validate + secret + anti-drift protocols and `references/` pointers.
- Any ` ```toml ` example block in the file is a valid, loadable config snippet.
- Tests covering discoverability + body constraints **(REQUIRED)** — implemented in task_06; this task's deliverable is the content those tests assert against.

## Tests
(The Rust test module is built in task_06; this task delivers the content it validates.)
- Unit tests:
  - [ ] The frontmatter parses into a `SkillManifest` with `name == "atelier-config-setup"` and a non-empty `description`. (task_06)
  - [ ] The body length is under the 500-line budget. (task_06)
- Integration tests:
  - [ ] Any ` ```toml ` block embedded in `SKILL.md` loads via the config loader without unknown-key/enum errors. (task_06)
- Test coverage target: >=80% (of the asserted content surface)
- All tests must pass

## Success Criteria
- All tests passing (via task_06)
- The skill is discoverable as `atelier-config-setup` with valid frontmatter, body under 500 lines.
- The wizard, self-validate (with degradation), secret, and anti-drift protocols are all present and point into `references/`.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean (no Rust changes here, but the gate must stay green).
