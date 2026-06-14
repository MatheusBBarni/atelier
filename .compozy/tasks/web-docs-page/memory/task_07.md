# Task Memory: task_07.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Emit 4 static Astro endpoints reading `getCollection('docs')`: `llms.txt`, `llms-full.txt`, per-page `.md` twin, `sitemap.xml`. All URLs absolute (site + `/atelier` base), all prerendered.

## Important Decisions
- Pure surface-building logic lives in `web/src/lib/machineSurfaces.mjs` (plain JS, no TS) so it is importable by both the `.ts` endpoints AND `node --test` unit tests (repo has no JS test framework / TS loader). Endpoints stay thin: fetch collection, normalize to `{id,title,nav_order,llms_optional,body}`, call the builders.
- llms.txt grouping: schema has no `area` field, so non-optional entries collapse into one `## Docs` section + `## Optional` bucket from `llms_optional`. Note text = first non-heading paragraph of body, truncated. (Follow-up: add an `area` frontmatter field if richer grouping is wanted.)
- Twin + llms-full prepend `# {title}` (title lives in frontmatter, not `entry.body`) so the markdown is a complete citable doc.
- Twin route is `[...slug].md.ts` (rest param, mirrors existing `[...slug].astro`) to handle nested ids like `_generated/configuration`.

## Learnings
- task_06 Governance page NOT yet created in this branch (only quickstart.md + concepts.md exist). Endpoints are generic so unaffected; bucketing/exclusion proven via a self-contained unit test, not Governance content.
- Absolute URL = `context.site.origin` + normalized BASE_URL + `docs/<id>/`. `context.site` is the configured `site`; `import.meta.env.BASE_URL` is `/atelier/` under GITHUB_PAGES else `/`.

## Files / Surfaces
- NEW: web/src/lib/machineSurfaces.mjs, web/src/pages/llms.txt.ts, web/src/pages/llms-full.txt.ts, web/src/pages/docs/[...slug].md.ts, web/src/pages/sitemap.xml.ts, web/tests/machineSurfaces.test.mjs

## Errors / Corrections

## Ready for Next Run
- Done. 4 endpoints + pure builders + 8 unit tests + dist check script all green (both base modes). `## Optional` is currently empty (no page sets `llms_optional`); fixture-driven unit test proves bucketing/exclusion.
