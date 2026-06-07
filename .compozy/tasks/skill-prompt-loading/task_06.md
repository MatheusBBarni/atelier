---
status: pending
title: "Project Skill-Loaded Feedback And Update Help Text"
type: docs
complexity: medium
dependencies:
  - task_03
  - task_04
  - task_05
---

# Task 06: Project Skill-Loaded Feedback And Update Help Text

## Overview
Add the visible user feedback for successfully loaded skills and update command wording so `/skill:<name>` is described as loading skill context. This task completes the metadata-only observability surface without turning chat projection or docs into a storage location for full skill bodies.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST project `skills_loaded` history events as concise user-visible chat items.
- MUST display loaded skill names and source origins/paths without displaying full `SKILL.md` content.
- MUST preserve first-use skill order in projected feedback.
- MUST avoid duplicate display rows for duplicate requested aliases that resolve to one canonical skill.
- MUST update TUI help text so `/skill:<skill_name>` communicates skill-context loading, not raw prompt prefixing.
- MUST update README command wording to match the TUI help language.
- MUST prove producer payloads remain metadata-only; chat projection must not be treated as the primary scrubber for full skill bodies.
- MUST include leakage regressions for history, debug logs, run records, and runtime envelope boundaries.
</requirements>

## Subtasks
- [ ] 6.1 Add explicit `skills_loaded` handling to chat projection.
- [ ] 6.2 Render concise loaded-skill feedback with names and source metadata.
- [ ] 6.3 Add projection safeguards for duplicate aliases and malformed payloads.
- [ ] 6.4 Update TUI help text and related tests.
- [ ] 6.5 Update README command wording for `/skill:<skill_name>`.
- [ ] 6.6 Add producer-side and projection-side leakage regression tests.

## Implementation Details
Use the TechSpec "Monitoring and Observability" and ADR-003 metadata-only decision. `record_event` mirrors payloads to history, debug logs, chat projection, and UI events, so the producer should emit only metadata before projection sees the event.

### Relevant Files
- `src/app/chat/projection.rs` - Adds explicit `skills_loaded` event handling and projection tests.
- `src/app/chat/mod.rs` - May need updates if a new chat item kind or status is introduced.
- `src/tui/mod.rs` - Updates help modal wording and tests that assert help text.
- `README.md` - Updates command documentation to match the new behavior.
- `src/app/mod.rs` - Produces `skills_loaded` metadata and run records that must not leak full skill bodies.
- `src/history/mod.rs` - Persists history and debug event payloads directly.
- `src/runtime/mod.rs` - Provides the runtime prompt envelope where rendered skill text should appear.

### Dependent Files
- `src/skills/mod.rs` - Provides loaded skill metadata contract used by app events and projection.
- `.multiagent` session history and run records - Must contain metadata only for loaded skills.
- `src/runtime/codex.rs`, `src/runtime/claude.rs`, `src/runtime/cursor.rs`, `src/runtime/zai.rs`, `src/runtime/fake.rs` - Continue to receive rendered text only through existing runtime prompt handling.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Requires concise run/history feedback for loaded skills.
- [ADR-002: Select Deterministic Common Flow For PRD](adrs/adr-002.md) - Requires visible evidence and fail-closed behavior.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Requires metadata-only `skills_loaded` and no full body persistence.

## Deliverables
- Chat projection for metadata-only `skills_loaded` events.
- Updated TUI help wording and README command wording.
- Projection tests for loaded skill display.
- Leakage regression tests covering history, debug logs, run records, projection, and runtime prompt boundary.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for full skill-load observability **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `skills_loaded` with one skill renders one concise standalone chat item titled like `Skills loaded`.
  - [ ] `skills_loaded` with multiple skills renders in first-use order.
  - [ ] Duplicate aliases in `requested_names` do not create duplicate visible rows.
  - [ ] Malformed payload fields named `content`, `body`, or carrying a sentinel full skill body are not displayed by projection.
  - [ ] TUI help rendering includes `/skill:<skill_name>` wording for loading skill context.
  - [ ] TUI help rendering no longer describes `/skill:<skill_name>` as only prefixing a prompt.
- Integration tests:
  - [ ] Valid skill prompt records `skills_loaded` metadata and no full `SKILL.md` body in `events.jsonl`.
  - [ ] Valid skill prompt with debug enabled records metadata but no full `SKILL.md` body in debug logs.
  - [ ] Run record JSON does not contain a sentinel full skill body.
  - [ ] Chat projection item displays skill names and source metadata without full skill body text.
  - [ ] Runtime prompt envelope contains rendered skill section exactly once, proving full text appears only at the runtime boundary.
  - [ ] README command wording matches the TUI help wording for `/skill:<skill_name>`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Users can see which skills loaded and where they came from.
- Help and README wording match the implemented `/skill:` behavior.
- Full skill bodies appear only in runtime prompts, not persisted observability surfaces.
