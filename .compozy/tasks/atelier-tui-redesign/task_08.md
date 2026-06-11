---
status: pending
title: "Surface polish: dropdowns, dialogs, help modal, input composer"
type: frontend
complexity: low
dependencies:
  - task_03
---

# Task 8: Surface polish: dropdowns, dialogs, help modal, input composer

## Overview

Apply the semantic theme deliberately to every remaining chrome surface so each role reads distinctly (PRD F2): agent/skill dropdowns, help modal, input composer, clarification composer/status, and the roster/chat block chrome. Task_03 already moved these to tokens mechanically; this task assigns the *right* tokens per surface, resolving the legacy yellow-overload by design rather than by substitution.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. Each surface MUST map to a deliberate token role: focused-input chrome (`border_focused`), transient overlays/dropdowns (consistent overlay treatment), informational modal (help), decision surfaces (clarification) — one visual language, no two surfaces sharing a color for unrelated reasons (PRD goal: one color = one meaning).
2. The full inventory from exploration MUST be covered: agent dropdown (:1465-1543), skill dropdown incl. tag colors (:1545-1652), help modal + `centered_rect` (:1843-1929), input composer block (:1376-1382), clarification composer (:2198-2250), clarification status (:2252-2270), roster block (:1346-1351), chat block (:1423-1427), legacy chat lines (:1931-1952).
3. Selected-item treatment in dropdowns and clarification options MUST share one convention (accent background with contrast foreground).
4. Skill tag colors (project vs personal, currently LightGreen/LightBlue) MUST map to distinct theme tokens preserving the distinction.
5. No layout, text, or interaction changes — styling only; the 13 existing dropdown/modal tests MUST pass with unchanged content assertions.
</requirements>

## Subtasks
- [ ] 8.1 Define the surface→token mapping table (one comment block in the render module or the theme module docs).
- [ ] 8.2 Apply to both dropdowns (chrome + selected/unselected + skill tags).
- [ ] 8.3 Apply to help modal and `centered_rect` backdrop.
- [ ] 8.4 Apply to input composer and clarification composer/status.
- [ ] 8.5 Apply to roster/chat block chrome and legacy chat lines.
- [ ] 8.6 Verify the 13 existing surface tests; add styled-cell assertions for the selected-item convention.

## Implementation Details

All sites are in `src/tui/mod.rs` and were tokenized in task_03 — this task edits which token each site uses, guided by the mapping table. The exploration notes carry the exact per-surface inventory with current colors; treat that as the checklist. See TechSpec "Component Overview" (Migration row) and PRD "User Experience".

### Relevant Files
- `src/tui/mod.rs` — full surface inventory: agent dropdown (:1465-1543), skill dropdown (:1545-1652), help modal (:1843-1929), input composer (:1376-1382), clarification composer/status (:2198-2270), roster (:1346-1351), chat block (:1423-1427), legacy lines (:1931-1952), `render_input_status` (:2037-2100).
- `src/tui/theme.rs` — token vocabulary; extend with overlay/selection tokens ONLY if the existing set cannot express the mapping (YAGNI).

### Dependent Files
- `src/tui/mod.rs` tests — 13 dropdown/help tests (:2729-3166) plus `renders_pending_approval_prompt` (:2663) exercise these surfaces.

### Related ADRs
- [ADR-003: Web Palette as Canonical Brand Source](../adrs/adr-003.md) — accent semantics.
- [ADR-002: Unified Single-Release Rollout](../adrs/adr-002.md) — phase 3 (remaining surfaces).

## Deliverables
- Deliberate token mapping applied to all nine surface groups; mapping table documented.
- Unit tests with 80%+ coverage of any new mapping helpers **(REQUIRED)**
- Integration tests for selection-convention styling **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Surface mapping helper (if introduced) returns the documented token per surface role.
- Integration tests:
  - [ ] All 13 existing dropdown/help/clarification tests pass with unchanged content assertions.
  - [ ] Selected dropdown item renders the shared selection treatment (styled-cell assertion on bg token) in both agent and skill dropdowns.
  - [ ] Skill tags render two distinct token styles for project vs personal sources.
  - [ ] Help modal and clarification composer borders use different tokens than the input composer (one-color-one-meaning spot check).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- No surface shares a token with an unrelated role (mapping table is the auditable artifact).
- Source-invariant test still green.
