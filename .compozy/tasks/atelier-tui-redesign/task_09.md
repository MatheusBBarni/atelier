---
status: pending
title: README assets and 3-terminal release verification
type: docs
complexity: low
dependencies:
  - task_04
  - task_06
  - task_07
  - task_08
---

# Task 9: README assets and 3-terminal release verification

## Overview

Produce and publish the distribution deliverables the whole feature exists to enable (PRD F6): a welcome-screen screenshot and a parallel-agents GIF in the README hero, plus the manual 3-terminal compatibility verification. This gates the announcement (ADR-002) — it ships only when every surface matches the assets.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. README.md MUST gain a hero section (after the title, before Features at :6) with the welcome screenshot and the parallel-agents GIF; the README currently contains zero images (verified).
2. Assets MUST live in `web/public/images/` alongside the existing 8 PNGs (the established asset home) and be referenced with paths that render on GitHub.
3. The GIF MUST show ≥2 agents running in parallel with visibly distinct accent colors (the PRD differentiator); the screenshot MUST show the welcome screen at ≥80 columns with the full wordmark.
4. The manual release checklist MUST be executed and recorded in the PR description: Terminal.app (256-color), iTerm2, Alacritty, plus a `NO_COLOR=1` run — each verifying welcome, footer, dropdowns, and a parallel run.
5. The 4 stale TUI screenshots referenced by the website (`web/src/pages/index.astro:132-157`: run-surface, skill-picker, agent-picker, help) MUST be flagged: either refresh them in this task's asset session (same capture setup, low marginal cost) or file the follow-up explicitly — PRD lists website *changes* as out of scope, but knowingly shipping stale brand imagery needs an owner decision recorded in the PR.
6. CONTEXT.md TUI-surface descriptions (:187-202) SHOULD be reviewed for accuracy after the redesign (welcome item, footer) and updated if they describe superseded behavior.
</requirements>

## Subtasks
- [ ] 9.1 Capture the welcome screenshot (≥80 cols, truecolor terminal, fresh session in a git repo so the facts box is full).
- [ ] 9.2 Record the parallel-agents GIF (multi-agent run, accents visible, footer in frame).
- [ ] 9.3 Add the hero section to README.md with both assets.
- [ ] 9.4 Execute the 3-terminal + NO_COLOR checklist; record results.
- [ ] 9.5 Decide and record the website-screenshot refresh (do now vs. filed follow-up).
- [ ] 9.6 Review CONTEXT.md surface descriptions; update welcome/footer mentions.

## Implementation Details

Asset capture happens against the finished product (all four dependency tasks merged). The npm launcher README (`npm/package/README.md`) is independent and needs no changes (verified). The release workflow packages README.md into archives (`.github/workflows/release.yml:154,166`) — broken image paths would ship; verify paths render from the GitHub repo view, not just locally.

### Relevant Files
- `README.md` — hero insertion point (:1-23); currently no images, no badges.
- `web/public/images/` — asset destination (8 existing PNGs set the convention).
- `web/src/pages/index.astro` — stale-screenshot inventory (:132-157, :183, :357).
- `CONTEXT.md` — TUI surface descriptions (:187-202, :265-266).

### Dependent Files
- `.github/workflows/release.yml` — packages README into release archives (:154, :166).
- `.github/workflows/pages.yml` — deploys `web/` if website screenshots are refreshed.

### Related ADRs
- [ADR-002: Unified Single-Release Rollout](../adrs/adr-002.md) — assets gate the announcement.
- [ADR-003: Web Palette as Canonical Brand Source](../adrs/adr-003.md) — brand consistency the assets must demonstrate.

## Deliverables
- README hero with welcome screenshot + parallel-agents GIF; assets in `web/public/images/`.
- Executed 3-terminal + NO_COLOR checklist with recorded results.
- Website-screenshot decision recorded; CONTEXT.md reviewed.
- Verification evidence in PR **(REQUIRED — this task's "tests" are the manual checklist)**
- Markdown link check on changed docs **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Not applicable (docs/assets task) — gate is the manual checklist below; no code paths added.
- Integration tests:
  - [ ] README image paths resolve in the GitHub rendered view (both assets visible).
  - [ ] Terminal.app (256-color): welcome, footer, dropdowns, parallel run render legibly — recorded pass.
  - [ ] iTerm2 (truecolor): same surfaces — recorded pass.
  - [ ] Alacritty: same surfaces — recorded pass.
  - [ ] `NO_COLOR=1` run: no wordmark, all content present, no color output — recorded pass.
  - [ ] GIF shows ≥2 simultaneously running agents with distinct accents.
- Test coverage target: >=80% (not applicable to assets; code coverage unchanged)
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80% (unchanged — no code in this task)
- README assets match the shipped product exactly (PRD phase-3 criterion).
- 3/3 terminals + NO_COLOR checklist recorded as passing.
- Website-screenshot decision documented in the PR.
