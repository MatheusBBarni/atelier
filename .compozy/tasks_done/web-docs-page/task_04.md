---
status: completed
title: "Write the Quickstart page"
type: docs
complexity: medium
dependencies:
  - task_03
---

# Write the Quickstart page

## Overview

Author the Quickstart — the activation north star — as a committed Markdown entry in the
docs collection. It follows the lazy + fake-first flow: install → optional credential-free
`fake` preview → connect a real runtime → a read-only first run → a first *approved write*
that surfaces the approval prompt as the safety "aha" → next steps (PRD F1, ADR-002).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST author `web/src/content/docs/quickstart.md` with `{title, nav_order}` frontmatter, following the Quickstart flow in TechSpec "User Experience" (Eli's flow).
- MUST present the credential-free `fake` runtime preview BEFORE requiring any real credential, and frame `fake` explicitly as a zero-setup preview (not a real run).
- MUST show a read-only first run, then a first *approved write* that surfaces the approval prompt, introducing the control-plane model at the moment of use.
- MUST include 1–2 real, copy-paste recipes inline (no separate recipes catalog).
- Every command and flag shown MUST match the actual CLI (verified against `src/cli.rs` / README), and `ZAI_API_KEY` / `codex login` prerequisites MUST be stated accurately.
- All in-page links MUST be `BASE_URL`-relative and link onward to Concepts and Governance.
</requirements>

## Subtasks
- [x] 4.1 Write the "before you begin" 3-bullet prerequisites (terminal, project dir, one of: agent-CLI login / API key / nothing → `fake`).
- [x] 4.2 Install + `atelier --doctor` verification step.
- [x] 4.3 The zero-setup `fake` preview of the loop.
- [x] 4.4 Connect a real runtime/key, then a read-only first run.
- [x] 4.5 The first approved write that surfaces the approval prompt (the safety "aha").
- [x] 4.6 1–2 inline recipes + "where to go next" links to Concepts and Governance.
- [x] 4.7 Verify every shown command against the real CLI.

## Implementation Details

New committed file `web/src/content/docs/quickstart.md`. See TechSpec "User Experience" for
the exact flow ordering and PRD F1. Source the install commands from the README and verify
flags against `src/cli.rs`; the credential reality (orchestrator needs `ZAI_API_KEY`,
workers need `codex login`) comes from the built-in defaults.

### Relevant Files
- `README.md:41-79` — install and quick-start commands to adapt.
- `src/cli.rs:17-50` — the CLI flags shown must exist here.
- `src/config/mod.rs:629-765` — default agents/runtimes that define the credential prerequisites.

### Dependent Files
- `web/src/pages/llms*.ts` and the `.md` twin endpoint (task_07) — include this page.
- `web/src/content/docs/concepts.md`, `governance.md` (task_05/06) — cross-linked targets.

### Related ADRs
- [ADR-002: V1 docs product approach](../adrs/adr-002.md) — the lazy/fake-first Quickstart decision.
- [ADR-004: Docs site as all-Markdown content collections](../adrs/adr-004.md) — page authored as a collection entry.

## Deliverables
- `quickstart.md` implementing the lazy/fake-first flow with 1–2 inline recipes and onward links.
- Commands verified against the real CLI.
- Build + link + accuracy verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Not applicable (prose); covered by build + link + command-accuracy checks below.
- Integration tests:
  - [x] The page builds and renders through `DocsLayout`; its `nav_order` places it first.
  - [x] Every command shown (`atelier --doctor`, the `fake`-runtime invocation, the first-run prompt) corresponds to a real flag/path in `src/cli.rs` / config.
  - [x] All in-page links (to Concepts and Governance) are `BASE_URL`-relative (`../concepts/`, `../governance/`); they resolve once those sibling pages (task_05/06) land.
  - [ ] Manual: on a cold machine the documented steps reach a real orchestrated run within the time target. (Deferred — per-release manual check; cannot run in this environment.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The Quickstart presents the fake preview before credentials and the approved-write "aha".
- Every command is accurate; all links are base-correct.
