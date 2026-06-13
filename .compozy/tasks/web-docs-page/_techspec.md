# TechSpec — Atelier Documentation Site (`/docs`)

## Executive Summary

This spec implements the PRD's five-page `/docs` section as **Markdown Astro content
collections** rendered by one shared layout, with **reference content generated from the
Rust source** by a new `atelier --emit-docs` subcommand, and **machine-readable twins**
(`llms.txt`, `llms-full.txt`, per-page `.md`) emitted as static endpoints. The build runs
the generator **at build time** in a custom GitHub Pages workflow (no committed
artifacts); a Rust coverage test plus a PR `web-checks` link gate enforce completeness and
base-path correctness.

The two primary trade-offs: (1) building reference into the binary as a build-time
subcommand trades a **heavier CI pipeline** (a Rust toolchain in the web deploy) for
**zero-drift, single-source** reference; (2) authoring everything as Markdown trades the
**pixel-identical `.command-table` styling** for **uniform, free `.md` twins and
`llms-full.txt`**. Both follow ADR-003/004/005. The work is split into the PRD's two
waves: Wave 1 (prose + foundation + `llms.txt`) is pure Node/Astro and ships on the
*existing* deploy; Wave 2 introduces the generator and the custom Rust-in-CI pipeline.

## System Architecture

### Component Overview

| Component | New/Modified | Responsibility | PRD features |
|---|---|---|---|
| `atelier --emit-docs <dir>` (Rust, `src/docgen/`) | New | Read merged config, `Cli::command()`, and `slash_commands::catalog()` in-process; write Markdown reference fragments | F6, F7, F8 |
| `web/src/content/docs/` collection + `content.config.ts` | New | One collection holding committed prose (`quickstart`, `concepts`, `governance`) + generated reference (`configuration`, `cli`) | F1, F2, F3, F7, F8 |
| `DocsLayout.astro` + `Base.astro` | New | Render every doc entry to HTML; shared head/fonts/nav/footer extracted from `index.astro` | F3, F4 |
| Endpoints: `llms.txt.ts`, `llms-full.txt.ts`, `docs/[slug].md.ts`, sitemap | New | Emit the curated index, the full concatenation, raw per-page twins, and the sitemap | F5 |
| `web/src/pages/index.astro` | Modified | Adopt `Base.astro`; add base-aware "Docs" nav link | F4 |
| `.github/workflows/pages.yml` | Modified (Wave 2) | Custom build: Rust → `--emit-docs` → `astro build` → deploy | F9 |
| `.github/workflows/web-checks.yml` | New | PR gate: build + lychee link-check (Wave 2: also generate) | F9 |
| `README.md` | Modified | Fix the "Optional" runtimes wording | F10 |

**Data flow:** at build, `atelier --emit-docs` writes `configuration.md` and `cli.md` into
the git-ignored `_generated/` area of the `docs` collection; Astro's `glob()` loader
merges them with the committed prose; `DocsLayout` renders HTML; the endpoints re-read the
same collection to emit `.md` twins, `llms-full.txt`, and `llms.txt`. The `atelier` binary
is the single source of truth; the site is a projection.

## Implementation Design

### Core Interfaces

The project is Rust + TypeScript (no Go). The primary Rust type other components depend on
— the generator entry point, mirroring `codemap::run_codemap`:

```rust
// src/cli.rs — new flag (mirrors `--codemap`), validated in run_cli_with's bail! guards
#[arg(long, value_name = "DIR")]
pub emit_docs: Option<PathBuf>,

// src/docgen/mod.rs — new module
pub struct DocgenSummary {
    pub written: Vec<PathBuf>, // generated Markdown fragments
}

/// Emits Markdown reference fragments into `out_dir`. Deterministic:
/// stable ordering, no timestamps, no live runtime probes.
pub fn emit_docs(config: &EffectiveConfig, out_dir: &Path) -> Result<DocgenSummary>;
```

The Astro content-collection schema other pages and endpoints depend on:

```ts
// web/src/content.config.ts
import { defineCollection, z } from "astro:content";
import { glob } from "astro/loaders";
const docs = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/docs" }),
  schema: z.object({
    title: z.string(),
    nav_order: z.number(),
    llms_optional: z.boolean().default(false),
  }),
});
export const collections = { docs };
```

### Data Models

- **`DocgenSummary`** (Rust): the list of written fragment paths, for the CLI summary and
  tests.
- **Generated Markdown fragments**: `configuration.md` (from the `PrintableConfig`
  builder, refactored to emit Markdown; `[agents]`, `[runtimes]`, `[council]`, `[limits]`,
  `[ui]`, `[workspace]`), `cli.md` (from `Cli::command()` walking `get_arguments()`), and
  the slash-command table inside `cli.md` (from `catalog()`). Each carries `title` /
  `nav_order` frontmatter.
- **`docs` collection entry**: `{ title, nav_order, llms_optional, body }`. No database;
  all state is static files. `[presets.*]` content is hand-written into the
  `configuration` source (from the `--init-config` template), since it is absent from the
  merged config.

### API Endpoints

Static, prerendered under base `/atelier`:

| Method · Path | Produces | Content |
|---|---|---|
| GET `/llms.txt` | `text/plain` | H1 + summary + categorized links; non-essential pages under `## Optional` |
| GET `/llms-full.txt` | `text/plain` | Concatenated raw Markdown of all non-`llms_optional` entries |
| GET `/docs/<slug>.md` | `text/markdown` | Raw Markdown twin of one page |
| GET `/sitemap.xml` | `application/xml` | All doc routes |

CLI surface: `atelier --emit-docs <dir>` → writes fragments, prints a one-line summary,
exits 0. Subject to the same mutual-exclusion guards as `--codemap` / `--print-config`.

## Integration Points

- **The `atelier` binary** — a *build dependency* of the site in Wave 2; the web CI builds
  and runs it. Failure surfaces as a failed build (fail-closed).
- **GitHub Pages** — deploy target; base path `/atelier` via `GITHUB_PAGES=true`. All links
  use the `BASE_URL` helper.
- **lychee** — link checker invoked over `web/dist` in CI; scoped to internal links to
  avoid external rate-limit flakiness.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/config/mod.rs` | Modified | Factor the `PrintableConfig` builder to also emit Markdown; **medium** (shared with `--print-config`) | Refactor; keep existing config tests green |
| `src/cli.rs` | Modified | New `--emit-docs` flag + `bail!` guard + handler after `--print-config`; **low** | Add field, guard, dispatch |
| `src/docgen/` | New | The generator module; **medium** | Implement, mirror `codemap` |
| `src/slash_commands.rs` | Unchanged | Read via `catalog()`; no `Serialize` needed; **none** | None |
| `web/src/content/`, `layouts/`, `pages/docs/`, endpoints | New | The docs section; **medium** | Implement |
| `web/src/pages/index.astro` | Modified | Extract `Base.astro`, add Docs nav link; **low-medium** (don't regress landing) | Careful refactor + visual check |
| `.github/workflows/pages.yml` | Modified (Wave 2) | Custom Rust+generate+build deploy; **medium** | Rewrite |
| `.github/workflows/web-checks.yml` | New | PR build + link gate; **low** | Add |
| `README.md` | Modified | Fix "Optional" runtimes wording; **low** | Edit |

## Testing Approach

### Unit Tests

- **Generator coverage (Rust):** over a default `EffectiveConfig`, assert `emit_docs`
  output contains **every** slash command (`catalog()`), **every** CLI long flag
  (`Cli::command().get_arguments()`), and every config section (`[agents]`…`[workspace]`).
  A new flag/command fails CI until documented.
- **Determinism (Rust):** two `emit_docs` runs are byte-identical (stable ordering; no
  timestamps; doctor's live output excluded).
- **Config-builder regression:** existing `--print-config` redaction/format tests must stay
  green after the `PrintableConfig` refactor.

### Integration Tests

- **Web build:** `atelier --emit-docs` then `astro build` (with `GITHUB_PAGES=true`)
  produces all five pages, `llms.txt`, `llms-full.txt`, per-page `.md` twins, and the
  sitemap.
- **Link-check:** lychee over `web/dist` passes (internal links resolve under `/atelier`) —
  the base-path 404 guard.
- **Manual (per release):** the Quickstart is validated on a cold machine to reach a real
  run; ≥5 LLM-citation spot-checks.

## Development Sequencing

### Build Order

**Wave 1 — pure Node/Astro; ships on the existing `pages.yml`:**

1. **README accuracy fix** (F10) — no dependencies.
2. **Extract `Base.astro`** from `index.astro` (head/fonts/nav/footer) (F4) — no
   dependencies; verify the landing page is visually unchanged.
3. **Docs collection + `DocsLayout` + three committed prose pages** (Quickstart, Concepts,
   Governance) (F1–F3) — depends on step 2.
4. **Machine-readable endpoints** (`llms.txt`, `llms-full.txt`, `docs/[slug].md`, sitemap)
   + base-aware **"Docs" nav link** (F5 Wave-1) — depends on step 3.
5. **New `web-checks.yml`** PR gate: `npm ci && npm run build` + lychee (Node-only) (F9
   Wave-1) — depends on step 4.

**Wave 2 — introduces the generator + the custom Rust-in-CI pipeline:**

6. **`src/docgen/` + `atelier --emit-docs`** (config/CLI/commands Markdown emitters;
   refactor the `PrintableConfig` builder) (F6) — depends on the frontmatter/dir contract
   fixed in step 3.
7. **Generator coverage test** (Rust) (F6 verification) — depends on step 6.
8. **Generated reference pages** (`configuration.md`, `cli.md`) into the collection + a
   `web/package.json` `generate`/`prebuild` script for local parity (F7, F8) — depends on
   steps 6 and 3.
9. **Rewrite `pages.yml`** to the custom Rust→generate→build deploy and **extend
   `web-checks.yml`** with the generate step (F9 Wave-2) — depends on steps 6 and 8.

### Technical Dependencies

- A Rust toolchain available in the web CI (Wave 2 only).
- The generated content directory path + frontmatter contract fixed before steps 6/8.
- The `PrintableConfig` refactor must preserve `--print-config` output (guarded by existing
  tests).

## Monitoring and Observability

A static site with no runtime telemetry (no analytics, per PRD). Operational signals are
CI-only:

- **Pages deploy status** and **`web-checks` (build + lychee) status** per PR/deploy.
- **Generator coverage + determinism tests** in the Rust test job.
- The generator prints a **summary of written fragments** (mirroring
  `codemap::render_summary`).
- **Per-release manual checks:** Quickstart cold-run; LLM-citation spot-check (the PRD's
  instrument-free metrics).

## Technical Considerations

### Key Decisions

- **Generator as a Rust `--emit-docs` subcommand, run at build time** (ADR-003). Rationale:
  in-process access to config/clap/catalog, no parsing, mirrors `--codemap`. Trade-off: a
  Rust toolchain in web CI. Rejected: a Node parsing script; committed fragments + drift
  gate.
- **All-Markdown content collections with derived twins** (ADR-004). Rationale: uniform
  rendering; `.md` twins + `llms-full.txt` for free; zero new deps. Trade-off: themed
  Markdown tables instead of `.command-table`. Rejected: hybrid JSON+`.astro`; MDX.
- **Custom Pages build + PR link gate** (ADR-005). Rationale: fresh generation, 404s caught
  pre-merge. Trade-off: heavier pipeline. Rejected: keep `withastro/action` + commit;
  link-check only.

### Known Risks

- **`PrintableConfig` refactor regresses `--print-config`** (medium) → keep the builder
  behavior identical; rely on existing redaction/format tests.
- **Base-path 404s** (medium) → the `BASE_URL` helper everywhere + the lychee gate.
- **Rust-in-CI flakiness/slowness on deploy** (medium) → cache cargo; reuse one release
  build across generate + deploy.
- **Doctor non-determinism** (low) → excluded from generated docs; Troubleshooting
  hand-written.
- **Markdown-table theming drift from the brand** (low) → doc-specific CSS tokens; visual
  review.

## Architecture Decision Records

- [ADR-001: V1 docs — derive reference from the Rust source and ship alongside the
  README](adrs/adr-001.md) — Generate reference; hand-write prose; ship alongside the
  README; defer the flip + RAG to V2.
- [ADR-002: V1 docs product approach — differentiation-led activation surface](adrs/adr-002.md)
  — Lead with Governance + Concepts + a lazy/fake-first Quickstart + first-class
  `llms.txt`; two waves; instrument-free metrics.
- [ADR-003: Reference generator — a Rust `--emit-docs` subcommand executed at build
  time](adrs/adr-003.md) — In-process generation from config/clap/catalog; no committed
  artifacts; coverage test.
- [ADR-004: Docs site as all-Markdown Astro content collections with derived machine-readable
  twins](adrs/adr-004.md) — One collection, one layout, endpoints for `.md` twins /
  `llms-full.txt` / `llms.txt`; no new deps.
- [ADR-005: CI/CD — custom GitHub Pages build with build-time generation + a PR web-checks
  link gate](adrs/adr-005.md) — Rust-in-CI deploy; lychee PR gate; local parity script.
