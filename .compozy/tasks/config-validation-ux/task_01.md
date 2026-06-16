---
status: completed
title: "Relocate edit_distance to a shared util module and add suggest_nearby_name"
type: refactor
complexity: medium
dependencies: []
---

# Task 01: Relocate edit_distance to a shared util module and add suggest_nearby_name

## Overview
Move the private `edit_distance` helper out of the skills module into a new neutral `src/util.rs` leaf, and add a `suggest_nearby_name` helper both `config` and `skills` can call for did-you-mean diagnostics. This avoids a `config → skills` layering inversion and creates the single shared home the config typo-hint task (task_02) depends on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `src/util.rs` exposing `pub fn edit_distance(left: &str, right: &str) -> usize`, moved verbatim from `src/skills/mod.rs`.
- MUST add `pub fn suggest_nearby_name<'a>(unknown: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str>` returning the single closest within-threshold candidate (threshold `2.max(unknown.len() / 3)`) or `None`.
- MUST register the module with `pub mod util;` in `src/lib.rs`.
- MUST update `src/skills/mod.rs` to import `crate::util::edit_distance` and remove its private copy, preserving the existing `skill_name_suggestions` behavior and thresholds.
- MUST NOT change the observable behavior of skill name suggestions.
</requirements>

## Subtasks
- [x] 1.1 Create `src/util.rs` with the moved `edit_distance` and the new `suggest_nearby_name`.
- [x] 1.2 Declare `pub mod util;` in the crate root module list.
- [x] 1.3 Repoint `skills` to the shared `edit_distance` and delete the private definition.
- [x] 1.4 Add unit tests for `edit_distance` and `suggest_nearby_name`.

## Implementation Details
See TechSpec "Implementation Design → Core Interfaces" for the helper signatures and "Development Sequencing → Build Order" step 1. Keep `suggest_nearby_name` a pure leaf with no dependency on `config` or `skills` types. The threshold mirrors the skills convention so suggestion behavior stays consistent across the crate.

### Relevant Files
- `src/skills/mod.rs` — Holds the private `edit_distance` (~line 909) and `skill_name_suggestions` (~876-907) that must move/repoint.
- `src/lib.rs` — Flat `pub mod` list where `util` is registered.
- `src/util.rs` — (new) Leaf module for shared text helpers.

### Dependent Files
- `src/config/mod.rs` — task_02 imports `suggest_nearby_name` from here.

### Related ADRs
- [ADR-004: Shared util::edit_distance and additive near-miss config hints](adrs/adr-004.md) — Mandates the leaf-module placement over `pub`-in-skills.

## Deliverables
- New `src/util.rs` with `edit_distance` + `suggest_nearby_name`.
- `skills` repointed to the shared helper; private copy removed; crate compiles.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Behavior-parity verification for skill name suggestions via existing skills tests **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `edit_distance("codx", "codex")` returns 1; identical strings return 0; empty vs `"abc"` returns 3.
  - [x] `suggest_nearby_name("codx", ["codex","claude","cursor","zai"])` returns `Some("codex")`.
  - [x] `suggest_nearby_name("zzzzzz", ["codex","claude","cursor","zai"])` returns `None` (beyond threshold).
  - [x] When two candidates are within threshold, the single closest (smallest distance) is returned.
- Integration tests:
  - [x] Existing `skills` name-suggestion tests pass unchanged against the relocated `edit_distance`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `edit_distance` lives in `util`; both `config` and `skills` call it through `crate::util`.
- Skill name suggestion behavior is unchanged.
