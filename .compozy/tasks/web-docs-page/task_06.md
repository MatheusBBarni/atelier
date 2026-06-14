---
status: completed
title: "Write the Governance and Safety page"
type: docs
complexity: medium
dependencies:
    - task_03
---

# Write the Governance and Safety page

## Overview

Author the Governance & Safety page — the category's unclaimed differentiator. It is
hand-written and source-anchored, leading with "what an agent can and cannot touch" and "the
durable, replayable record", then the two-layer model (capabilities vs approval mode),
limits, untrusted-input handling, and an honest-limits note (PRD F2, ADR-002/004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST author `web/src/content/docs/governance.md` with `{title, nav_order}` frontmatter.
- MUST lead with "what an agent can and cannot touch" — the mediated `ActionRequest` set and read vs write roots — and "the durable, replayable record" (per-session history under `.multiagent/`).
- MUST explain the two layers: capabilities (what an agent profile may attempt) vs `ApprovalMode` (`yolo` default vs `normal` — when write/command actions surface an approval prompt).
- MUST cover limits and what happens at `LimitReached`, and MUST state that skills are guidance and do NOT bypass approvals/capabilities.
- MUST include an honest-limits note ("strong boundaries, not a guarantee").
- Every claim MUST be source-anchored (no over-promising); all links `BASE_URL`-relative.
</requirements>

## Subtasks
- [x] 6.1 Philosophy: the harness owns every action; agents only request.
- [x] 6.2 What an agent can and cannot touch: the action set + read/write roots.
- [x] 6.3 The two layers: capabilities vs `ApprovalMode` (with an action × capability × prompts-in-normal table).
- [x] 6.4 Limits and `LimitReached`.
- [x] 6.5 The durable, replayable record (audit/replay a run).
- [x] 6.6 Untrusted input + the skills-are-guidance disclaimer + an honest-limits note.

## Implementation Details

New committed file `web/src/content/docs/governance.md`. See TechSpec "User Experience" and
CLAUDE.md "Actions + capabilities". Source every claim from the action set, the approval
modes/capabilities, the workspace roots, and the limits in code — do not invent guarantees.

### Relevant Files
- `src/actions/mod.rs` — the `ActionRequest` set (ReadFile, ListFiles, SearchText, RunCommand, ApplyPatch, WriteFile, RecordNote).
- `src/config/mod.rs:17-23` (`ApprovalMode`), `:25-36` (`Capability`), `:170-193` (`[limits.*]`), workspace read/write roots.
- `src/skills/mod.rs` — the "skills are guidance, do not bypass approvals/permissions" disclaimer to quote.
- `CLAUDE.md` — the control-plane and event-sourcing/history description.

### Dependent Files
- `web/src/pages/llms*.ts` + the `.md` twin endpoint (task_07) — include this page (it is high-priority, non-Optional, in `llms.txt`).
- `web/src/content/docs/quickstart.md`, `concepts.md` — cross-linked.

### Related ADRs
- [ADR-002: V1 docs product approach](../adrs/adr-002.md) — Governance as the V1 differentiator.
- [ADR-004: Docs site as all-Markdown content collections](../adrs/adr-004.md) — page authored as a collection entry.

## Deliverables
- `governance.md` leading with the can/can't-touch promise and the durable record, with the two-layer model, limits, untrusted-input handling, and an honest-limits note.
- Source-anchored claims.
- Build + accuracy verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (prose); covered by build + source-accuracy checks below.
- Integration tests:
  - [x] The page builds and renders through `DocsLayout`.
  - [x] The documented action set matches the `ActionRequest` variants in `src/actions/mod.rs`.
  - [x] The documented approval modes (`yolo`/`normal`) and capability names match `src/config/mod.rs`.
  - [x] The skills-are-guidance disclaimer and an honest-limits note are present; no claim asserts an absolute guarantee.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The page leads with the can/can't-touch promise and the durable record; claims are source-anchored.
- The page renders and links are base-correct.
