---
status: pending
title: Per-agent accent colors and run-summary restyle
type: frontend
complexity: medium
dependencies:
  - task_03
---

# Task 7: Per-agent accent colors and run-summary restyle

## Overview

Give each configured agent a stable accent color (roster-order round-robin via `theme.accent_for`) applied consistently in the roster, chat item headers, and the agent dropdown, and restyle the run-summary view with theme tokens (PRD F4). This makes interleaved parallel-agent output visually attributable — the "Live Show" identity surface behind the README GIF.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. Accent assignment MUST be `theme.accent_for(roster_index)` where roster index is the agent's position in `state.agents` (order verified deterministic: orchestrator first, then alphabetical — `build_agent_views` :4559, `agent_roster_rank` :4659).
2. The roster MUST render each agent's name in its accent (replacing the uniform cyan at `src/tui/mod.rs:1306-1310`).
3. Chat item headers for agent-attributed kinds (`AgentProgress`, `AgentResult`) MUST use the owning agent's accent on the title span (`chat_item_header_line` :1757). Attribution: agent name is embedded in the title (`projection.rs:160-164` "{agent} {status}", `:893` "{agent}: {status}") — resolve roster index by matching the title's agent-name prefix against `state.agents`; items with no matching agent keep the default severity styling.
4. The agent dropdown (`agent_dropdown_item` :1513-1543) MUST show each agent's id/name in its accent so the selection UI teaches the color mapping.
5. `RunSummary` items MUST be restyled with theme tokens as the styled run conclusion (severity-driven, not agent-accented — a summary spans agents).
6. Accent mapping MUST be consistent across all three surfaces for the same agent in the same session (single helper, no duplicated lookup logic).
</requirements>

## Subtasks
- [ ] 7.1 Add an accent-resolution helper mapping an agent name → roster index → accent (one place, used by all surfaces).
- [ ] 7.2 Apply accents in the roster name spans.
- [ ] 7.3 Apply accents in `chat_item_header_line` for `AgentProgress`/`AgentResult` titles.
- [ ] 7.4 Apply accents in `agent_dropdown_item` id/name spans.
- [ ] 7.5 Restyle `RunSummary` header/body with theme tokens.
- [ ] 7.6 Tests: deterministic assignment, two-agent distinctness, unattributed-item fallback.

## Implementation Details

The attribution mechanism (title-prefix matching) is the pragmatic V1 choice given `ChatItemView` carries the agent name only in `title` (verified in projection.rs). If matching proves brittle during implementation, the fallback documented in exploration is extending `ChatSourceRef` with an optional agent id — flag it rather than silently switching. See TechSpec "Component Overview" and ADR-006.

### Relevant Files
- `src/tui/mod.rs` — roster loop (:1293-1352, name style :1306-1310), `chat_item_header_line` (:1757-1769), `agent_dropdown_item` (:1513-1543), test helpers `state_with_agent_roster` (:4083) and `agent_view` (:4093).
- `src/tui/theme.rs` — `accent_for` (task_02).
- `src/app/chat/projection.rs` — title formats binding agent names (:160-164, :893) — read-only reference for the matching rule.
- `src/app/mod.rs` — `build_agent_views` (:4559), `agent_roster_rank` (:4659) — ordering guarantee.

### Dependent Files
- `src/tui/mod.rs` tests — `roster_displays_streaming_status_as_running` (:2472), `renders_live_step_stream_detail_as_chat_progress` (:2493) exercise the touched render paths.
- `src/app/chat/mod.rs` — `ChatSourceRef` (:104-109) only if the documented fallback becomes necessary.

### Related ADRs
- [ADR-006: Round-Robin Agent Accents](../adrs/adr-006.md) — assignment policy and rejected alternatives.
- [ADR-002: Unified Single-Release Rollout](../adrs/adr-002.md) — phase 3 (orchestration identity).

## Deliverables
- Consistent per-agent accents across roster, chat headers, and dropdown; restyled run summary.
- Unit tests with 80%+ coverage of the accent-resolution helper **(REQUIRED)**
- Integration tests for multi-agent render attribution **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Accent resolution for a 3-agent roster returns pool colors 0, 1, 2 in roster order; a 7-agent roster wraps (index 5 == pool[5 % len]).
  - [ ] Resolving an agent name absent from the roster returns no accent (caller falls back to severity styling).
  - [ ] Title "fixer running" resolves to agent "fixer"'s index; title "fixer: done" (AgentResult format) resolves identically.
- Integration tests:
  - [ ] Two-agent state with `AgentProgress` items from each: rendered headers carry the two distinct accent styles (verify via styled-buffer cell inspection at the title cells).
  - [ ] Roster render for the same state shows the same two accents on the matching names.
  - [ ] A `RunSummary` item renders with theme tokens and no agent accent.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Parallel-run output is visually attributable per agent (PRD phase-3 criterion).
- Same agent shows the same accent in roster, chat, and dropdown within a session.
