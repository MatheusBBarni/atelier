---
status: pending
title: "Emit machine-readable surfaces (llms.txt, twins, sitemap)"
type: frontend
complexity: medium
dependencies:
  - task_03
  - task_04
  - task_05
  - task_06
---

# Emit machine-readable surfaces (llms.txt, twins, sitemap)

## Overview

Emit the machine-readable surfaces that let LLMs cite Atelier accurately: `llms.txt` (a
curated index with an `## Optional` bucket), `llms-full.txt` (the concatenated bodies of
non-Optional pages), a per-page raw-Markdown `.md` twin, and a sitemap. All are static Astro
endpoints that read the docs collection, so they reflect whatever pages exist
(PRD F5, ADR-004/001).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `web/src/pages/llms.txt.ts` endpoint emitting the llms.txt format: an H1, a blockquote summary, `##` sections of `[name](url): note` links grouped by area, and an `## Optional` bucket driven by each entry's `llms_optional` frontmatter flag.
- MUST add a `web/src/pages/llms-full.txt.ts` endpoint concatenating the raw Markdown of all non-`llms_optional` entries.
- MUST add a per-page raw-Markdown twin endpoint (e.g. `web/src/pages/docs/[slug].md.ts`) serving each page's source Markdown.
- MUST add a sitemap listing all doc routes, with NO new dependency (hand-rolled endpoint, not an integration).
- All emitted URLs MUST be absolute and carry the site + `/atelier` base; all endpoints MUST be prerendered to static files.
</requirements>

## Subtasks
- [ ] 7.1 Build the `llms.txt` endpoint (H1 + summary + grouped links + `## Optional`).
- [ ] 7.2 Build the `llms-full.txt` endpoint (concatenated non-Optional bodies).
- [ ] 7.3 Build the per-page `.md` twin endpoint.
- [ ] 7.4 Build the sitemap endpoint.
- [ ] 7.5 Verify all four outputs reflect the current collection and use base-correct absolute URLs.

## Implementation Details

New endpoint files under `web/src/pages/` reading `getCollection('docs')`. See TechSpec
"API Endpoints" for the route/content contract and "Core Interfaces" for the `llms_optional`
flag. Use the `BASE_URL`/site to build absolute URLs.

### Relevant Files
- `web/src/content.config.ts` (task_03) — the `docs` collection + `llms_optional` flag the endpoints read.
- `web/src/content/docs/{quickstart,concepts,governance}.md` (task_04–06) — the content the endpoints project.
- `web/src/pages/index.astro:1-7` — the `BASE_URL` pattern for absolute URLs.
- `web/astro.config.mjs` — `site` + `base` for absolute URL construction.

### Dependent Files
- `web/src/content/docs/_generated/*.md` (task_11) — generated reference flows into these endpoints automatically once in the collection.
- `.github/workflows/web-checks.yml` (task_08) — link-checks the emitted output.

### Related ADRs
- [ADR-004: Docs site as all-Markdown content collections](../adrs/adr-004.md) — the twins/llms surfaces.
- [ADR-001: Derive reference from source, ship alongside the README](../adrs/adr-001.md) — machine-readability as a V1 goal.

## Deliverables
- `llms.txt`, `llms-full.txt`, per-page `.md` twin, and sitemap endpoints.
- Outputs that reflect the docs collection with base-correct absolute URLs.
- Build + structure verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `llms.txt` output contains the required structure: a single H1, a blockquote summary, `##` link sections, and an `## Optional` section bucketing entries flagged `llms_optional`.
  - [ ] `llms-full.txt` contains the body of the Governance page and excludes any `llms_optional` entry.
  - [ ] The `.md` twin for a given slug returns that page's raw Markdown.
- Integration tests:
  - [ ] `astro build` prerenders `/llms.txt`, `/llms-full.txt`, `/docs/<slug>.md`, and the sitemap as static files.
  - [ ] Every URL emitted in `llms.txt` and the sitemap is absolute and carries the `/atelier` base when built with `GITHUB_PAGES=true`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The four machine-readable surfaces build, are valid, and reflect the collection.
- All emitted URLs are base-correct.
