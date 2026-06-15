---
status: completed
title: "Extract a shared Base.astro layout"
type: refactor
complexity: medium
dependencies: []
---

# Extract a shared Base.astro layout

## Overview

The site has no shared layout — the `<head>` (including the Google Fonts link), nav, and
footer are inline in `index.astro`, so any new page would duplicate them and drift. This
task extracts them into a reusable `web/src/layouts/Base.astro` and migrates the landing
page to use it, so the landing page and every docs page share one source (PRD F4, ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `web/src/layouts/Base.astro` containing the shared `<head>` (charset, viewport, theme-color, favicon, the Google Fonts `<link>`, the `global.css` import), the nav header, and the footer, with a default content slot.
- MUST accept page-level props for at least `title` and `description` (and an optional canonical/OG image).
- MUST migrate `web/src/pages/index.astro` to render its body through `Base.astro` with NO visual change to the landing page.
- MUST preserve the `assetPath()`/`BASE_URL` helper so all asset and link URLs stay correct under the `/atelier` base.
- MUST NOT change the landing page's content, copy, or styling.

</requirements>

## Subtasks
- [x] 2.1 Create `Base.astro` with the shared head, nav, footer, and a default slot.
- [x] 2.2 Parameterize `title`/`description` (and optional OG/canonical) via props.
- [x] 2.3 Move the `assetPath()`/`BASE_URL` helper into the layout (or a shared import) so every page reuses it.
- [x] 2.4 Migrate `index.astro` to wrap its sections in `Base.astro`.
- [x] 2.5 Verify the landing page renders identically (visual diff against the reference screenshots).

## Implementation Details

New `web/src/layouts/Base.astro`; modify `web/src/pages/index.astro` to consume it. See
TechSpec "Component Overview" (Base.astro / DocsLayout) and "Build Order" step 2. The head
block to extract is at `index.astro:163-178` (fonts/meta), the nav at `:189-234`, and the
`assetPath`/`BASE_URL` helper at `:1-7`.

### Relevant Files
- `web/src/pages/index.astro:1-7` — the `assetPath`/`BASE_URL` helper to share.
- `web/src/pages/index.astro:163-234` — head, fonts, and nav markup to extract.
- `web/src/styles/global.css` — `.nav`, `.brand`, `.nav__links`, font/`:root` tokens the layout relies on.
- `web/.verification/*.png` — reference screenshots for the visual-unchanged check.

### Dependent Files
- `web/src/pages/index.astro` — rewritten to render through `Base.astro`.
- `web/src/layouts/DocsLayout.astro` (task_03) — will compose `Base.astro`.

### Related ADRs
- [ADR-004: Docs site as all-Markdown Astro content collections](../adrs/adr-004.md) — mandates extracting `Base.astro` before adding pages.

## Deliverables
- `web/src/layouts/Base.astro` with shared head/nav/footer + slot + title/description props.
- `index.astro` migrated to use `Base.astro` with no visual change.
- Build + visual-parity verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (markup extraction); covered by build + visual parity below.
- Integration tests:
  - [ ] `astro build` succeeds and emits `index.html` containing the same `<title>`, theme-color meta, favicon, and Google Fonts link as before the change.
  - [ ] The built landing page is visually identical to the `web/.verification` reference screenshots (manual diff).
  - [ ] Asset URLs in the built output still carry the `BASE_URL` prefix when built with `GITHUB_PAGES=true`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `Base.astro` is the single source for head/nav/footer and is reused by the landing page.
- The landing page is visually unchanged.
