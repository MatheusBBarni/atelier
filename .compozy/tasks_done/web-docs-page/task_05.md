---
status: completed
title: "Write the Concepts page"
type: docs
complexity: low
dependencies:
  - task_03
---

# Write the Concepts page

## Overview

Author the Concepts page that gives an evaluator the mental model needed to trust and adopt
a control-plane tool: the orchestrator run loop, the run lifecycle states, the distinction
between agent profiles and runtimes, and the control plane at a conceptual level
(PRD F3, ADR-004). It is explanation, not reference — exhaustive keys live on the
Configuration page.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST author `web/src/content/docs/concepts.md` with `{title, nav_order}` frontmatter.
- MUST explain the orchestrator loop, the run lifecycle states (Idle → Planning → Running → WaitingForUser → Completed/Failed/Interrupted/LimitReached), and how routing picks a single agent, a council, or a parallel group.
- MUST explain the agent-profile vs runtime distinction (a profile carries capabilities/model; a runtime is the backing CLI/HTTP).
- MUST stay conceptual — no exhaustive config reference (defer to the Configuration page); descriptions MUST be accurate against the architecture.
- MUST link onward to Governance & Safety (for the trust detail) and Configuration; all links `BASE_URL`-relative.
</requirements>

## Subtasks
- [x] 5.1 Explain the run lifecycle states and what advances them.
- [x] 5.2 Explain orchestrator routing (single agent / council / parallel group).
- [x] 5.3 Explain agent profiles vs runtimes.
- [x] 5.4 Summarize the control plane and link to Governance for detail.
- [x] 5.5 Cross-link to Governance and Configuration.

## Implementation Details

New committed file `web/src/content/docs/concepts.md`. See TechSpec "User Experience" and
the CLAUDE.md architecture summary (run lifecycle, runtimes, agent profiles). Verify the
lifecycle state names against the orchestrator's `RunState`.

### Relevant Files
- `CLAUDE.md` — the architecture/run-lifecycle summary to ground the explanation.
- `src/orchestrator/mod.rs` — `RunState` lifecycle and `OrchestratorDecision` routing.
- `src/runtime/mod.rs` — `RuntimeKind` (the runtimes a profile can target).
- `src/config/mod.rs:629-765` — the built-in agent profiles referenced conceptually.

### Dependent Files
- `web/src/pages/llms*.ts` + the `.md` twin endpoint (task_07) — include this page.
- `web/src/content/docs/quickstart.md`, `governance.md` — cross-linked.

### Related ADRs
- [ADR-002: V1 docs product approach](../adrs/adr-002.md) — Concepts page added to serve the evaluator.
- [ADR-004: Docs site as all-Markdown content collections](../adrs/adr-004.md) — page authored as a collection entry.

## Deliverables
- `concepts.md` explaining the lifecycle, routing, profiles vs runtimes, and the control plane.
- Cross-links to Governance and Configuration.
- Build + accuracy verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (prose); covered by build + accuracy checks below.
- Integration tests:
  - [ ] The page builds and renders through `DocsLayout` with its `nav_order` placement.
  - [ ] The run-lifecycle state names match the orchestrator's `RunState` variants.
  - [ ] Onward links to Governance and Configuration resolve under the `/atelier` base.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The mental model (lifecycle, routing, profiles vs runtimes) is accurate and conceptual.
- The page renders and links are base-correct.
