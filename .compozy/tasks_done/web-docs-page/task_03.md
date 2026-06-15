---
status: completed
title: "Scaffold the docs content collection, layout, nav, and styles"
type: frontend
complexity: medium
dependencies:
  - task_02
---

# Scaffold the docs content collection, layout, nav, and styles

## Overview

This task lays the docs foundation every page renders through: an Astro content collection
for docs, a `DocsLayout` that renders an entry through `Base.astro` with a section nav and
in-page TOC, the `/docs` routes, a base-aware "Docs" link in the nav, and doc-specific CSS
(a readable heading scale plus themed Markdown tables/code). It must build with zero or more
entries so prose (task_04–06) and generated reference (task_11) can plug in later
(PRD F4, ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define a `docs` content collection in `web/src/content.config.ts` with a `glob()` loader over `src/content/docs` and the schema in TechSpec "Core Interfaces" (`title`, `nav_order`, `llms_optional`).
- MUST add `web/src/layouts/DocsLayout.astro` that renders a collection entry's HTML through `Base.astro`, with a section nav ordered by `nav_order` and an in-page table of contents.
- MUST add the `/docs` routes (a `/docs` index that lists sections by `nav_order`, and a dynamic entry route) using only `BASE_URL`-relative links.
- MUST add a base-aware "Docs" link to the nav in `Base.astro`.
- MUST add doc-sized heading styles plus themed Markdown table and code-block styles so generated reference tables match the brand (the marketing `h1`/`h2` scale is too large for doc body).
- The routes MUST build successfully when the collection has zero, one, or many entries.
</requirements>

## Subtasks
- [x] 3.1 Define the `docs` collection + schema in `content.config.ts`.
- [x] 3.2 Build `DocsLayout.astro` (composes `Base.astro`; adds section nav + TOC).
- [x] 3.3 Add the `/docs` index route and the dynamic entry route.
- [x] 3.4 Add the base-aware "Docs" nav link in `Base.astro`.
- [x] 3.5 Add doc CSS: heading scale, themed Markdown tables, code blocks.
- [x] 3.6 Verify the section builds and renders a sample entry through `DocsLayout`.

## Implementation Details

New files: `web/src/content.config.ts`, `web/src/layouts/DocsLayout.astro`,
`web/src/pages/docs/index.astro`, `web/src/pages/docs/[...slug].astro`; modify
`web/src/styles/global.css` (doc tokens) and `web/src/layouts/Base.astro` (nav link). See
TechSpec "Core Interfaces" (the `content.config.ts` schema) and "Component Overview". Reuse
existing primitives (`.section`, `.eyebrow`) and the `assetPath`/`BASE_URL` helper rather
than new patterns.

### Relevant Files
- `web/src/styles/global.css:700-741` — `.command-table` pattern and the section/heading tokens to theme doc Markdown against.
- `web/src/pages/index.astro:1-7` — the `assetPath`/`BASE_URL` helper.
- `web/src/pages/index.astro:203-207` — the `nav__links` block to add the "Docs" link to.
- `web/astro.config.mjs` — `base` handling for `/atelier`.

### Dependent Files
- `web/src/content/docs/*.md` (task_04–06) — prose entries authored into this collection.
- `web/src/pages/*.ts` endpoints (task_07) — read this collection.
- `web/src/content/docs/_generated/*.md` (task_11) — generated entries plug into this collection.

### Related ADRs
- [ADR-004: Docs site as all-Markdown Astro content collections](../adrs/adr-004.md) — the authoring/rendering model implemented here.

## Deliverables
- `content.config.ts` (the `docs` collection + schema), `DocsLayout.astro`, the `/docs` routes, the "Docs" nav link, and doc CSS.
- A building `/docs` section that renders a sample entry.
- Build + render verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Not applicable (Astro scaffolding); covered by build + render below.
- Integration tests:
  - [x] `astro build` (with `GITHUB_PAGES=true`) emits the `/docs/` index and a dynamic entry route from a sample collection entry.
  - [x] A sample entry with `{title, nav_order}` renders through `DocsLayout` with its TOC and the section nav ordered by `nav_order`.
  - [x] The "Docs" nav link resolves to `/atelier/docs/` under the GitHub Pages base (not `/docs/`).
  - [x] Doc headings and a sample Markdown table render with the themed doc styles, not the marketing hero scale.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The `/docs` section builds and renders collection entries through one shared layout.
- The "Docs" nav link is base-correct; the landing page is unaffected.
