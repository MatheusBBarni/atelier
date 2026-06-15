---
status: completed
title: "Add the web-checks PR workflow with link checking"
type: infra
complexity: medium
dependencies:
  - task_07
---

# Add the web-checks PR workflow with link checking

## Overview

There is no PR CI for the site today, and base-path `/atelier` links can 404 only in
production. This task adds a new PR-triggered `web-checks` workflow that builds the site
(Node-only at this stage) and link-checks the output with lychee, so broken internal links
and base-path 404s fail before merge (PRD F9, ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `.github/workflows/web-checks.yml`, triggered on pull requests touching `web/**` and the workflow file.
- MUST run `npm ci` + the Astro build with `GITHUB_PAGES=true`, then a lychee link-check over `web/dist`.
- MUST fail the PR on any broken internal link.
- SHOULD scope the link-check to internal links (or allowlist external hosts) to avoid external rate-limit flakiness.
- MUST use node 24 with `actions/setup-node` npm caching keyed to `web/package-lock.json`.
- MUST remain Node-only at this stage — the Rust generate step is added later (task_12).
</requirements>

## Subtasks
- [x] 8.1 Create the workflow with the PR trigger and `web/**` path filters.
- [x] 8.2 Add node 24 setup + `npm ci` + build (`GITHUB_PAGES=true`).
- [x] 8.3 Add the lychee link-check over `web/dist`, scoped to internal links.
- [x] 8.4 Confirm the gate fails on a seeded broken internal link and passes on a clean build.
- [x] 8.5 Document the local equivalent command.

## Implementation Details

New file `.github/workflows/web-checks.yml`. Mirror the trigger/path-filter style of
`pages.yml` and use `lychee` per TechSpec "Integration Points" and "Build Order" step 5.
There is no existing PR workflow to copy, so this establishes the pattern.

### Relevant Files
- `.github/workflows/pages.yml` — trigger/path-filter and node-version pattern to mirror.
- `web/package.json` — the `build` script; `web/package-lock.json` exists for `npm ci`.
- `web/astro.config.mjs` — `base` `/atelier` (the case lychee must catch).

### Dependent Files
- `.github/workflows/web-checks.yml` is extended in task_12 with the Rust generate step.

### Related ADRs
- [ADR-005: CI/CD — custom Pages build + a PR web-checks link gate](../adrs/adr-005.md) — this workflow.

## Deliverables
- `.github/workflows/web-checks.yml` that builds and link-checks the site on PRs.
- Internal-link scoping (or an external allowlist).
- Gate-behavior verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (CI config); covered by the gate-behavior checks below.
- Integration tests:
  - [ ] On a PR touching `web/**`, the workflow runs `npm ci` + build (`GITHUB_PAGES=true`) + lychee and passes on a clean build.
  - [ ] A deliberately broken internal `/docs/...` link (correct only without the base) FAILS the workflow.
  - [ ] An unreachable external link does NOT fail the workflow (internal-only scope / allowlist).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The PR gate builds the site and link-checks it, catching base-path 404s before merge.
- The workflow is Node-only at this stage.
