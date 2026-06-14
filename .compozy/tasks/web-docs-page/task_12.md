---
status: completed
title: "Custom Pages deploy and generator step in CI"
type: infra
complexity: high
dependencies:
  - task_08
  - task_10
  - task_11
---

# Custom Pages deploy and generator step in CI

## Overview

Replace the opaque Node-only `withastro/action` deploy with a custom Pages workflow that
builds the `atelier` binary, runs `--emit-docs`, then `astro build` and deploy; and extend
the `web-checks` PR workflow with the same Rust generate step so PRs build the full
generated site before link-checking. This is the final wiring for build-time generation —
and the highest-regression-risk change, since it rewrites the working deploy
(PRD F9 Wave-2, ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST rewrite `.github/workflows/pages.yml` build job to: checkout → set up Rust (`dtolnay/rust-toolchain@stable`) → build `atelier` and run `--emit-docs` into the content collection → set up node 24 → `npm ci` + `astro build` (`GITHUB_PAGES=true`) → upload artifact; the existing `deploy-pages` job MUST be preserved.
- MUST extend `.github/workflows/web-checks.yml` (task_08) with the Rust build + generate step before the Node build, so the link-check covers generated pages.
- SHOULD cache cargo (e.g., `Swatinem/rust-cache`) and reuse one release build across the generate steps.
- MUST preserve the `pages` concurrency group and the `github-pages` environment.
- MUST NOT regress the existing landing-page deploy.
</requirements>

## Subtasks
- [x] 12.1 Rewrite the `pages.yml` build job (Rust → `--emit-docs` → node → `astro build` → artifact).
- [x] 12.2 Keep the `deploy-pages` job and the concurrency/environment settings.
- [x] 12.3 Add the Rust build + generate step to `web-checks.yml`.
- [x] 12.4 Add cargo caching and reuse one binary build across steps.
- [x] 12.5 Verify a full deploy serves the landing page AND the generated `/docs` reference.
- [x] 12.6 Verify the PR gate now link-checks the generated pages.

## Implementation Details

Rewrite `.github/workflows/pages.yml` and extend `.github/workflows/web-checks.yml`
(from task_08). See TechSpec "Build Order" step 9 and "Integration Points". Use the repo's
existing Rust toolchain convention; cargo caching is net-new (no precedent).

### Relevant Files
- `.github/workflows/pages.yml` — the current `withastro/action` build + `deploy-pages` job to rewrite.
- `.github/workflows/release.yml` — `dtolnay/rust-toolchain@stable` + `cargo build --locked --release --bin atelier` precedent.
- `web/package.json` — the `generate`/`build` scripts from task_11 the workflow drives.

### Dependent Files
- None — this is the final task in the chain.

### Related ADRs
- [ADR-005: CI/CD — custom Pages build + PR web-checks link gate](../adrs/adr-005.md) — the deploy + gate design.
- [ADR-003: Reference generator — build-time generation](../adrs/adr-003.md) — why the pipeline needs Rust.

## Deliverables
- Rewritten `pages.yml` (Rust → generate → build → deploy) preserving the deploy job and concurrency/environment.
- Extended `web-checks.yml` with the Rust generate step.
- Cargo caching.
- A deploy that serves the landing page and the generated docs.
- Pipeline verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (CI config); covered by the pipeline checks below.
- Integration tests:
  - [ ] The rewritten `pages.yml` build job produces a `web/dist` that contains the landing page AND the generated Configuration/CLI pages plus the `llms.txt`/`llms-full.txt` surfaces.
  - [ ] The deployed site serves the generated `/docs` reference under the `/atelier` base.
  - [ ] On a PR, `web-checks` builds the binary, generates, builds the site, and link-checks the generated pages — failing on a generated-page base-path 404.
  - [ ] The landing-page output is unchanged versus the previous deploy.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Deploys are freshly generated and serve the full site (landing + generated docs).
- The PR gate covers generated pages; the landing-page deploy is not regressed.
