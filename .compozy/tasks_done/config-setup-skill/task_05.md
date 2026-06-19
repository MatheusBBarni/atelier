---
status: completed
title: sync-skills harness + generated discovery mirrors
type: infra
complexity: medium
dependencies:
  - task_01
  - task_02
  - task_03
---

# sync-skills harness + generated discovery mirrors

## Overview
Add `npm/scripts/sync-skills.mjs` that regenerates atelier's discovery mirrors from the canonical `skills/atelier-config-setup/`, plus a check mode that asserts each mirror is byte-equal to the canonical source. This makes the canonical skill the single source of truth (ADR-004) and gives CI (task_09) a drift gate so hand-edited mirrors can't silently diverge.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `npm/scripts/sync-skills.mjs` that copies `skills/atelier-config-setup/` → `.agents/skills/atelier-config-setup/` and `.claude/skills/atelier-config-setup/` as byte-identical trees (SKILL.md + references/).
- MUST add a `--check` mode that exits non-zero when any mirror differs from canonical (the CI equality gate), and exits 0 when in sync.
- MUST wire npm scripts in `npm/package.json` (e.g. `sync:skills` and `check:skills`) mirroring the existing `sync:versions` / `check:versions` convention, reusing `npm/scripts/common.mjs` helpers where applicable.
- MUST generate the mirrors and commit them so atelier's own runtime + `/skill:` dropdown and Claude-Code dogfooding discover `atelier-config-setup` (`.agents/skills` + `.claude/skills` are atelier discovery roots).
- MUST reconcile the existing `.claude/skills` symlink-to-`.agents/skills` convention: either generate real byte-copies in both (so mirror-equality holds for both) or document/limit the equality check to the dirs that are real copies — pick one and make the task_06 mirror-equality test + the `--check` mode agree with it.
- MUST NOT hand-edit the generated mirrors (they are outputs); the canonical `skills/` tree is the only edited source.
</requirements>

## Subtasks
- [x] 5.1 Implement `sync-skills.mjs` copy mode (canonical → `.agents/skills` + `.claude/skills`).
- [x] 5.2 Implement `--check` mode (byte-equality comparison; non-zero on drift).
- [x] 5.3 Add `sync:skills` / `check:skills` scripts to `npm/package.json`.
- [x] 5.4 Reconcile the `.claude/skills` symlink convention vs. byte-copy mirrors; document the chosen approach in the script header.
- [x] 5.5 Run the sync to produce the initial committed mirrors.

## Implementation Note
Chosen approach (5.4): `.agents/skills/atelier-config-setup/` is a **real byte-copy**;
`.claude/skills/atelier-config-setup` is a **symlink** to `../../.agents/skills/atelier-config-setup`,
matching the repo's existing `.claude/skills/<name>` symlink convention. The drift contract (used by
both `--check` and the task_06 mirror-equality test) is **content equality following symlinks**: every
canonical file must exist with identical bytes under each mirror path. A script-level test
(`npm/tests/sync-skills.test.mjs`, 4 cases) covers in-sync / altered / stray / symlink-follow.

## Implementation Details
Create `npm/scripts/sync-skills.mjs` modeled on `npm/scripts/sync-versions.mjs` (Node ESM, `import` from `npm/scripts/common.mjs` for `readJson`/`writeJson`/path helpers). The repo currently hand-mirrors skills and `.claude/skills` holds symlinks into `.agents/skills` (ADR-004 calls out the drift-prone status quo this replaces). The `--check` mode is what task_09 runs in CI. See TechSpec "System Architecture → Sync harness" and ADR-004.

### Relevant Files
- `npm/scripts/sync-versions.mjs`, `npm/scripts/common.mjs` — the script/helper precedent (`cargoPackageVersion`, `readJson`, `writeJson`).
- `npm/package.json` — `scripts` block (`sync:versions`, `check:versions`) to mirror.
- `skills/atelier-config-setup/` (task_01/02/03) — the canonical source to mirror.
- `.agents/skills/`, `.claude/skills/` — atelier discovery roots (`src/skills/mod.rs` `skill_roots()`); current `.claude/skills` symlink layout.

### Dependent Files
- `tests/atelier_config_skill.rs` (task_06) — mirror-equality test asserts the generated mirrors match canonical.
- `.github/workflows/release.yml` (task_09) — runs `check:skills`.

### Related ADRs
- [ADR-004: Canonical skill + generated mirrors, CI equality check](../adrs/adr-004.md)
- [ADR-002: Repo-bundle delivery (mirrors keep atelier's runtime independent)](../adrs/adr-002.md)

## Deliverables
- `npm/scripts/sync-skills.mjs` (copy + `--check` modes) and `sync:skills`/`check:skills` in `npm/package.json`.
- Generated `.agents/skills/atelier-config-setup/` and `.claude/skills/atelier-config-setup/` mirrors of the canonical source.
- Tests **(REQUIRED)**: the mirror-equality assertion (task_06) plus a script-level check that `--check` passes when synced.

## Tests
- Unit tests:
  - [ ] (Script) `node sync-skills.mjs --check` exits 0 immediately after a sync, and non-zero when a mirror file is altered. (npm test or manual harness)
- Integration tests:
  - [ ] `tests/atelier_config_skill.rs` mirror-equality test: each discovery mirror is byte-equal to `skills/atelier-config-setup/` for the dirs the chosen approach covers. (task_06)
  - [ ] atelier's skills-discovery lists `atelier-config-setup` from the generated mirror in a temp root. (task_06)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Mirrors are generated (not hand-edited) and `--check` cleanly detects drift; atelier discovers `atelier-config-setup`.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
