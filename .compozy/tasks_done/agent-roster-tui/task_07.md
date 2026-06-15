---
status: completed
title: "Accent-by-identity consistency (roster/chat/dropdown) + strengthened contract tests"
type: frontend
complexity: medium
dependencies:
  - task_06
---

# Task 07: Accent-by-identity consistency (roster/chat/dropdown) + strengthened contract tests

## Overview

Make agent accent follow identity across all three surfaces (roster, chat, dropdown) so the `NeedsInput` pin can never recolor an agent. Presently, accents are resolved at render-time via positional indices — when Task 06's `NeedsInput` pin reorders a row, the agent's accent shifts and breaks its link to the chat transcript. This task ensures accents are anchored to canonical agent identity, not display position, by storing `accent_index` on the unified `RosterRow` view-model and repointing all three surfaces to read from that canonical index. Strengthen the two contract tests to assert accent persists under a pinned-reorder fixture.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. MUST identify the three canonical accent surfaces and verify each reads `accent_index` from identity: roster row rendering (`src/tui/mod.rs:2114-2177`), chat title styling (`item_agent_accent` at `3115`), and `/agent:` dropdown list items (`2488`).
2. MUST add a code comment documenting the single-source rule at each of the three render sites, referencing ADR-005 and this task.
3. MUST ensure `accent_index` is computed in `build_roster_rows` from the canonical (sorted) agent order **before** the `NeedsInput` pin-sort, so pinning never perturbs the index. Use `agent_roster_rank` (orchestrator-first, then alphabetical) to establish the canonical position.
4. MUST repoint chat and dropdown accent lookups from render-time positional indices to the canonical `accent_index` — they should read from `RosterRow.accent_index` or derive it the same way (from agent.id lookup in the canonical-sorted agents list).
5. MUST update two contract tests to use a pinned-reorder fixture: `roster_names_carry_same_accents_as_chat` and `agent_dropdown_ids_carry_same_accents_as_roster`. Each test MUST assert that when an agent is pinned to the top (e.g., via `NeedsInput`), its roster accent equals its canonical `theme.accent_for(canonical_index)`, not `accent_for(0)`.
6. SHOULD use `title_cell_fg` (at `tui/mod.rs:8741`) for cell-level color assertions in the updated tests, as it is robust to multi-byte glyphs.
7. MUST document the risk that a future fourth accent surface deriving accent independently would silently break the single-source contract — recommend adding it as a CI invariant or comment in the codebase.
</requirements>

## Subtasks

- [x] 7.1 Verified `accent_index` on `RosterRow` is computed from canonical order before the pin-sort (task_03 builder; unit test `needs_input_pin_preserves_canonical_accent_index`). Single-source-rule comments added at the three sites.
- [x] 7.2 Roster already reads `row.accent_index` (task_06); added the single-source-rule comment (ADR-005) at `roster_row_item`.
- [x] 7.3 Chat `item_agent_accent` already resolves by canonical identity (`agent_index_for_title`); added the canonical single-source doc block here.
- [x] 7.4 `/agent:` dropdown already resolves by canonical `id` lookup; added the single-source-rule comment.
- [x] 7.5 `roster_names_carry_same_accents_as_chat` uses the pinned-reorder fixture (done task_06; NeedsInput fixer pinned to row 0 → `accent_for(1)`).
- [x] 7.6 `agent_dropdown_ids_carry_same_accents_as_roster` strengthened: renders roster + dropdown together under the pin; asserts fixer = `accent_for(1)` in both surfaces.
- [x] 7.7 Single-source rule + fourth-surface risk documented as a doc block on `item_agent_accent` (cross-referenced from the other two sites).

## Implementation Details

### Relevant Files

- `/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs` — contains `RosterRow` struct (Task 06), `AgentView` (id and canonical order), `build_roster_rows`, and `build_agent_views` which establishes the canonical sort via `agent_roster_rank`.
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/mod.rs` — three accent surfaces: roster render block (2114-2177), `item_agent_accent` / `agent_index_for_title` (3105-3123), `/agent:` dropdown (2488-2495); two contract tests (8828, 8870); `title_cell_fg` helper (8741); `work_indicator_active` / `RunState` for determining when to pin.
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/theme.rs` — `Theme::accent_for(index)` method; `AGENT_ACCENT_COUNT` constant.

### Dependent Files

- `/Users/matheusbbarni/projects/multiagent-harness/.compozy/tasks/agent-roster-tui/adrs/adr-005.md` — canonical identity index decision, repointing logic, single-source rule.
- `/Users/matheusbbarni/projects/multiagent-harness/.compozy/tasks/agent-roster-tui/_techspec.md` — "System Architecture", "Core Interfaces" (RosterRow definition), "Data Models", "Integration Points", "Impact Analysis" sections covering the accent surfaces and test updates.

### Related ADRs

- [ADR-005: Accent-by-Identity Decoupling](../adrs/adr-005.md) — primary decision: canonical-order `accent_index` on `RosterRow`, repoint all three surfaces, strengthen contract tests with pinned-reorder fixture, risk of a fourth surface.
- [ADR-001: V1 Mechanism and Scope](../adrs/adr-001.md) — constraint 1 (accent colors must remain consistent under pin) and item 5 (accent-by-identity decoupling).

## Deliverables

- Roster render block rewritten to consume `row.accent_index` (no enumerate index).
- Chat title styling (`item_agent_accent`) repointed to canonical identity index.
- `/agent:` dropdown accent lookup repointed to canonical identity index.
- Single-source rule comments added at all three sites, cross-referenced to ADR-005 and this task.
- `roster_names_carry_same_accents_as_chat` updated to a pinned-reorder fixture and passing.
- `agent_dropdown_ids_carry_same_accents_as_roster` updated to a pinned-reorder fixture and passing.
- Unit tests with 80%+ coverage **(REQUIRED)** — specifically, the updated contract tests asserting `accent_index` persistence under reorder.
- Integration test snapshot validating the three surfaces show consistent accent under pinned state **(REQUIRED)**.
- Documentation of the fourth-surface risk (CI invariant or codebase comment).

## Tests

### Unit Tests

- **Canonical identity index computation**: `accent_index` on `RosterRow` matches the agent's position in the canonical (orchestrator-first, alphabetical) sort, regardless of its activity or pin status. Verify with `build_roster_rows` directly and a hand-built agent list.
- **Pin does not recolor**: when an agent is pinned to the top (e.g., `activity: NeedsInput`), its `accent_index` does not change compared to the unpinned state. Fixture: `[orchestrator(Idle, accent_index=0), explorer(NeedsInput, accent_index=1)]` → after pin-sort, order is `[explorer(NeedsInput), orchestrator(Idle)]` but `explorer.accent_index` remains `1`.
- **`roster_names_carry_same_accents_as_chat` with pinned-reorder**: create a state with `[explorer(accent_for(0)), fixer(accent_for(1))]` in canonical order; set `fixer` to `NeedsInput` so it pins to the top; render and assert via `title_cell_fg` that the *roster* line "Fixer" has color `accent_for(1)` (canonical), not `accent_for(0)` (display position), and matches the *chat* title for "fixer" which resolves via `agent_index_for_title`.
- **`agent_dropdown_ids_carry_same_accents_as_roster` with pinned-reorder**: same fixture; render the `/agent:` dropdown and assert the pinned agent's color in the dropdown matches its canonical `accent_index`, not its display rank in the dropdown list.
- **Consistent across all three surfaces**: render state with one pinned agent and verify `title_cell_fg` reports the same color for the agent's name in all three locations (roster, chat title, dropdown).

### Integration Tests

- **Snapshot: roster/chat/dropdown accent consistency (pinned state)**: render a state with two agents (canonical order: orchestrator, explorer) where explorer is pinned; verify the pixel-perfect output shows explorer in the same color in all three surfaces and that color is `theme.accent_for(1)`, not `accent_for(0)`.
- **Snapshot: all three surfaces with multiple agents and varied activity states**: render idle/active/stalled agents, verify each maintains its canonical color across roster, chat, and dropdown regardless of activity.
- **NO_COLOR snapshot**: with `TerminalCaps{ no_color: true }`, all accents resolve to `Color::Reset`; verify the fixture still passes and states are disambiguated by glyph/label, not color.
- **Test coverage target: >=80%** across the accent-resolution logic (`build_roster_rows` accent computation, the three render-site lookups, the two contract tests).
- **All tests must pass** (both existing tests updated and new pinned-reorder tests added).

## Success Criteria

- All tests passing, including the two updated contract tests with pinned-reorder fixtures.
- Test coverage >=80% for accent-identity logic.
- Every pinned agent renders with its canonical `theme.accent_for(canonical_index)` across all three surfaces (roster, chat, dropdown), verified via snapshot and `title_cell_fg` assertions.
- Code comments at the three accent surfaces clearly document the single-source rule, referencing ADR-005.
- The fourth-surface risk is documented as a codebase comment or CI invariant, discouraging independent accent resolution at future sites.
- No regression: existing unpinned agents maintain their current colors; the toggle, footer, and roster/chat linking remain unchanged.
