# Task Memory: task_08.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Done. Added PR-triggered `web-checks.yml` (Node-only): npm ci → build (`GITHUB_PAGES=true`) → `check:surfaces` → lychee internal-link gate. Catches base-path 404s before merge (F9, ADR-005). Rust generate step deferred to task_12.

## Important Decisions
- Link-check uses `lycheeverse/lychee-action@v2` with `--offline` (internal-only scope; external URLs never checked → no rate-limit flakiness) and `fail: true`.
- Base-path handling: `astro build` emits at dist root but Pages serves under `/atelier`. Stage `dist` into `_site/atelier/`, then `lychee --root-dir <ws>/_site <ws>/_site/atelier`. A link missing the base (`/docs/...`) resolves to `_site/docs/...` → not found → fails (the 404 guard). Verified locally.
- `.md` LLM twins are DROPPED from the link-check copy (`find _site/atelier -name '*.md' -delete`): their relative links target rendered HTML routes, not sibling files, so checking them as files is false noise. They are validated by `check:surfaces` instead.
- Recurse the staged directory (no `**` glob in args) to avoid relying on shell globstar in the action runner; lychee recurses dirs natively.
- Local parity (subtask 8.5): `npm run check:links` (`web/scripts/check-links.mjs`) mirrors CI exactly. Requires lychee installed + a prior `GITHUB_PAGES=true npm run build`.

## Learnings
- On THIS branch the clean build legitimately has 7 broken internal links: prose links to `../configuration/` (Wave 2 generated) and `../governance/` (task_06) — pages not yet present. This is the known forward-reference condition (shared memory). The gate will be RED until Governance + Configuration/CLI land; that is correct behavior, not a defect, and was NOT worked around (no allowlist — ADR-005 chose strictness).
- Verified gate behavior with lychee 0.24.2: clean (stub the 2 missing pages) → 0 errors/exit 0; current/broken → exit 2; unreachable external → Excluded under `--offline`.
- Repo CI action convention (release.yml): `actions/checkout@v4`, `actions/setup-node@v4`, `node-version: "24"`, `cache: npm`, `cache-dependency-path`.

## Files / Surfaces
- `.github/workflows/web-checks.yml` (new) — the PR gate. Extended in task_12 with the Rust generate step.
- `web/scripts/check-links.mjs` (new) + `web/package.json` `check:links` script (new).
- `web/.gitignore` — added `_site/` (staging dir created by the local script).

## Errors / Corrections
- None blocking. Note: `GITHUB_PAGES=true` must be a step-level env for BOTH the build and `check:surfaces` steps (an inline `VAR=x cmd && othercmd` only scopes the env to the first command).

## Ready for Next Run
- task_12 extends this workflow: add Rust toolchain + `atelier --emit-docs` before `npm run build`, and rewrite `pages.yml`. The `configuration`/`cli` forward-ref links resolve once the generator emits those pages.
