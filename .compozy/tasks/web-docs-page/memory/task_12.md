# Task Memory: task_12.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Final CI wiring for build-time docs generation. Rewrote `pages.yml` (Node-only `withastro/action` → custom Rust→generate→build→deploy) and extended `web-checks.yml` with the same Rust generate step so the PR link gate covers generated pages. Deploy job, `pages` concurrency, and `github-pages` environment preserved.

## Important Decisions
- **Single generate, explicit step.** Build the binary once (`cargo build --locked --release --bin atelier`), set `ATELIER_BIN=${{ github.workspace }}/target/release/atelier` on a dedicated `npm run generate` step, then build with `npx astro build` (NOT `npm run build`). Using `npx astro build` skips the `prebuild` hook so generate does not run twice — the explicit step already produced the fragments. This is the clean non-redundant pattern; `ATELIER_BIN` makes generate reuse the release build instead of `cargo run`.
- **Cargo cache:** `Swatinem/rust-cache@v2` (net-new, no repo precedent) after `dtolnay/rust-toolchain@stable`. Toolchain action matches `release.yml`.
- **Path filters (ADR-005):** added generator sources `src/cli.rs`, `src/config/**`, `src/slash_commands.rs` to BOTH workflows' triggers (web-checks per ADR explicit note; pages.yml too so a CLI/config/slash change redeploys fresh docs — keeps published reference from drifting).
- Scoped commit to my two workflow files only (parallel WIP in `src/app/mod.rs` + other task files left untouched).

## Learnings
- `npm run build` triggers the `prebuild` hook (= generate). To avoid a double generate when an explicit generate step exists, invoke `npx astro build` directly.
- Local pipeline repro mirrors CI exactly: `ATELIER_BIN=… npm run generate && GITHUB_PAGES=true npx astro build` → `dist/` has landing `index.html` + `docs/configuration/`, `docs/cli/` + `llms.txt`/`llms-full.txt`; `check:surfaces` reports base `/atelier/`.
- Link gate (12.6) confirmed covering generated pages: generated configuration/cli pages link-check clean; only remaining errors are `/docs/governance` (task_06, not on this branch) — expected RED per shared memory.

## Files / Surfaces
- `.github/workflows/pages.yml` — rewritten build job; deploy/concurrency/environment preserved.
- `.github/workflows/web-checks.yml` — Rust toolchain+cache+build+generate steps inserted before the Node build; `npm run build` → `npx astro build`.

## Errors / Corrections
- None.

## Ready for Next Run
- Final task in the chain; no dependents. The lychee gate will go fully green once task_06's `/docs/governance` page lands.
