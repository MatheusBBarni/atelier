# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Extract shared `Base.astro` layout (head/fonts/nav/footer + slot) and migrate `index.astro` to use it with zero visual change. Done.

## Important Decisions
- Landing nav is **hero-embedded** (`<header class="nav">` lives inside `<section class="hero">`, overlaying the absolute `.hero__image` via `z-index:2`). A body-top nav would break the landing visually, so the nav is NOT forced at body top.
- Resolution: nav markup extracted to `src/components/Nav.astro` (single source). `Base.astro` renders it via `renderHeader` prop (default `true`, for future docs pages); landing passes `renderHeader={false}` and places `<Nav />` inside its hero — output stays identical.
- `index.astro` has **no footer** today; adding one would be a visual change. So `Base.astro` renders `<footer>` only when a `footer` named slot is provided (`Astro.slots.has("footer")`). Landing provides none.
- GitHub-star `<script is:inline>` kept in `index.astro` (landing-specific), not moved into `Nav.astro`.
- Brand link kept as `href="#top"` (not `assetPath("/")`) to preserve landing behavior — docs nav can parameterize later.

## Learnings
- Verify visual parity by token-diffing built `dist/index.html` against a pre-change baseline: `diff <(tr -s ' \n\t' '\n' <baseline) <(tr -s ' \n\t' '\n' <new)`. Result here: identical except whitespace between final `</body></html>` (inert). CSS is content-hashed — unchanged hash = unchanged styles.

## Files / Surfaces
- New: `web/src/lib/basePath.ts` (shared `basePath`/`assetPath`), `web/src/components/Nav.astro`, `web/src/layouts/Base.astro`.
- Modified: `web/src/pages/index.astro` (wraps body in `<Base>`, imports shared `assetPath`, drops inline helper + own `<head>`/global.css import).

## Errors / Corrections
- First wrote `Nav.astro` brand href as `assetPath("/")`; reverted to `#top` to avoid changing the landing page.

## Ready for Next Run
- task_03 (DocsLayout) composes `Base.astro`: use `assetPath` from `../lib/basePath`; default `renderHeader` gives a top nav, or override the `head`/`footer` named slots. `Base.astro` props: `title`, `description`, optional `canonical`, `ogImage`, `renderHeader`.
