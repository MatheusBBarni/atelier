# Task Memory: task_03.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Scaffold the `/docs` section: `docs` content collection + schema, `DocsLayout.astro` (via `Base.astro`, section nav + TOC), `/docs` index + dynamic entry routes, base-aware "Docs" nav link, themed doc CSS. Builds with zero/one/many entries. Done + verified.

## Important Decisions
- "Docs" nav link added to `Nav.astro` (not `Base.astro` body) — task_02 moved nav markup into `Nav.astro`; `assetPath` already imported there.
- No committed sample doc entry: prose is task_04–06's scope. Verified rendering with throwaway entries, then removed them. Committed `src/content/docs/.gitkeep` so the authoring dir ships ready.
- DocsLayout fetches `getCollection('docs')` itself for the section nav (single ordering source); receives `entry` + `headings` as props, renders `<Content/>` via default slot.
- Doc scale via `.docs__title` / `.doc-prose h2,h3` classes — class specificity (0,1,x) beats the marketing element rules `h1`/`h2` (0,0,1), so the doc body escapes the clamp(5.2–12rem) hero scale without touching marketing CSS.

## Learnings
- Astro 6 content layer caches the glob data store in `node_modules/.astro/data-store.json` (plus `web/.astro/`). Deleting a source `.md` is NOT enough to drop a stale route — clear `node_modules/.astro` (and `.astro`) before a zero-entry rebuild or the removed entry's route keeps generating.
- `web/.gitignore` already covers `dist/` and `.astro/`. Build is robust to the content dir being absent entirely (fresh checkout) — exit 0.
- Astro 6 render API: `import { getCollection, render } from "astro:content"`; `render(entry)` → `{ Content, headings }`; dynamic route via `getStaticPaths` mapping `entry.id` → `params.slug` with `[...slug].astro`.

## Files / Surfaces
- New: `web/src/content.config.ts`, `web/src/layouts/DocsLayout.astro`, `web/src/pages/docs/index.astro`, `web/src/pages/docs/[...slug].astro`, `web/src/content/docs/.gitkeep`.
- Modified: `web/src/components/Nav.astro` (Docs link), `web/src/styles/global.css` (doc CSS block appended at EOF).

## Errors / Corrections
- First zero-entry rebuild still emitted `/docs/_sample/` from the cached data store; fixed by clearing `node_modules/.astro` (see Learnings).

## Ready for Next Run
- Collection + layout contract is fixed for task_04–06 prose (frontmatter: `title`, `nav_order`, optional `llms_optional`) and task_11 generated `_generated/*.md`.
- Follow-up (out of scope): shared `Nav.astro` still uses landing-only anchor links (`#features`/`#commands`/`#install`); on `/docs` pages those don't resolve. Revisit nav-on-docs behavior when prose lands.
