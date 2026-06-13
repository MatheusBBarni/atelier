# Atelier Documentation Site (`/docs`)

## Overview

Atelier has a strong marketing landing page but no documentation. A developer
convinced by the landing page has nowhere to go to actually install, run, and
configure the tool — and a developer (or their LLM) searching *"how do I configure a
runtime in atelier"* finds nothing authoritative. This idea adds a small multi-page
`/docs` section to the existing Astro site that takes a first-time evaluator from
install to a successful orchestrated run in under five minutes, and gives hands-on
configurers an accurate reference for `multiagent.toml`, the CLI, and the slash
commands.

It serves two audiences on one surface: **Evaluator Eli** (just found Atelier; gives it
~5 minutes to prove itself) and **Configurer Cora** (already running it; wiring
runtimes, models, agents, presets, limits). A third, non-human reader matters too — the
**LLMs** developers now ask first: for a niche tool they haven't memorized,
machine-readable docs are how Atelier gets cited correctly instead of hallucinated.

The V1 is a **Strategic Bet** scoped with discipline — five pages, plus the two pieces
that make it compound: reference content **generated from the Rust source** (so it
can't drift) and an `llms.txt` corpus. It ships **alongside** the existing README, which
stays the canonical, never-404 storefront until a proven deploy earns the migration.

## Problem

Atelier's landing page sells the product well, then dead-ends. There is no install
walkthrough, no command reference, no configuration guide — the GitHub README is the
only documentation, and it is a single ~296-line scroll mixing install, CLI, config,
agents, runtimes, and project layout. An evaluator who wants to try the tool must read
the whole thing; a configurer who wants one fact (the default for `limits.agent_steps`,
the fields of an `[agents.*]` block) must search it. The research is blunt about the
cost: 93% of developers call incomplete or outdated documentation a pervasive problem,
documentation is a top-4 factor before adopting a tool, and 52% cite "lack of
documentation" as the single biggest blocker. For a tool whose whole pitch is *control*,
the absence of a setup-and-safety guide is a credibility gap.

The reference content also **already drifts**. The command tables hand-maintained in
`web/src/pages/index.astro` are stale against the Rust source of truth — missing
`/help`, `/workflow`, `/queue`, and the `@` file picker that `src/slash_commands.rs`
defines. This is not a discipline failure to fix by trying harder; it is the predictable
result of hand-copying a surface whose truth lives in code. Any documentation that
re-types commands, flags, and config keys by hand is born drifting, and a small team
cannot keep it honest.

Finally, discovery of niche tools has changed. The top of the funnel is increasingly an
LLM answer, and 66% of developers say their #1 frustration with AI tools is output that
is "almost right, but not quite." For a tool the models have little training data on,
that failure mode is acute: ask an assistant how to configure Atelier's Z.ai runtime and
it will guess. Without a machine-readable, authoritative corpus (`llms.txt`, clean
headings, a sitemap), Atelier cedes its own onboarding narrative to hallucination.

### Market Data

- **93%** of developers say incomplete/outdated docs are a pervasive problem; **60%** of
  contributors rarely or never write docs. — GitHub Open Source Survey (5,500+).
- Documentation is a **top-4** factor before adopting a tool/API; **52%** call "lack of
  documentation" the biggest blocker. — Postman State of the API.
- **66%** of developers' #1 AI-tool frustration is "almost right, but not quite." —
  Stack Overflow 2025 Developer Survey (49,000+).
- Structured onboarding correlates with **~50%** higher retention; "time to first
  success" is the canonical activation metric. — developer-onboarding research (GitLab,
  Document360).
- The AI-agent docs category is a **Mintlify monoculture** (Goose, Claude Code, CrewAI,
  Cline, LangGraph); none document a governance/safety story — an unclaimed
  differentiator.

## Summary / Differentiator

Every comparable tool documents *capabilities*; almost none document *governance*.
Atelier is terminal-native and control-plane-first — the harness owns file edits,
commands, approvals, capabilities, read/write roots, limits, and a durable, replayable
record. A first-class **Governance & Safety** page ("the agent cannot touch your
filesystem except through gated, logged actions") is a trust surface no competitor
offers, and it answers the developer anxiety about AI accuracy head-on. Combined with
reference that **can't drift** and a corpus **LLMs cite correctly**, Atelier's docs
differentiate on exactly the axis the category ignores.

## Core Features

| #   | Feature                              | Priority | Description                                                                                                                                                                                  |
| --- | ------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1  | Quickstart page                      | Critical | install → `atelier --doctor` → first orchestrated run in < 5 min, with 1-2 real copy-paste recipes seeded inline. The activation north star.                                                |
| F2  | Source-derived reference generator   | Critical | Build-time step emitting structured data from existing Rust surfaces (`--print-config`, `--doctor --json`, the `slash_commands.rs` catalog) to feed the reference pages. Ends the drift bug class. |
| F3  | Configuration reference page         | High     | `multiagent.toml` — agents, presets, runtimes, council, limits, ui — generated from `src/config/mod.rs` via F2. Serves Configurer Cora.                                                     |
| F4  | Commands & CLI reference page        | High     | Slash commands + CLI flags and their validation rules, generated from `slash_commands.rs` and `cli.rs` via F2.                                                                              |
| F5  | Governance & Safety page             | High     | Hand-written, source-anchored: approval modes, capabilities, read/write roots, limits, the durable event-sourced record. The category's unclaimed differentiator.                          |
| F6  | Shared docs foundation               | High     | Extract `Base.astro` (head, fonts, nav, footer); add a docs nav + sticky TOC, base-aware (`assetPath`) cross-page links, and a doc-sized heading scale. Prerequisite for the section.       |
| F7  | Discoverability assets               | High     | `llms.txt` + `llms-full.txt` + sitemap, plus question-shaped headings, so LLMs and search engines cite Atelier accurately.                                                                  |
| F8  | Troubleshooting page                 | Medium   | Thin, seeded only from the 2-3 known first-run failures (missing API key, runtime CLI not installed, config won't parse). Grows from real issues.                                           |
| F9  | CI link-check gate                   | Medium   | lychee/linkinator over `dist/` in the Pages workflow so base-path `/atelier` 404s fail the deploy, not the user.                                                                            |

## KPIs

| KPI                          | Target                                                                                                                          | How to Measure                                                                |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Doc coverage vs Rust source  | 100% of CLI flags, slash commands, config sections, agents, runtimes; **0 drift items at launch**                              | Launch checklist cross-referencing `cli.rs`, `slash_commands.rs`, `config/mod.rs` |
| Time to First Successful Run | Median **< 5 min** (Quickstart → completed run)                                                                                 | Quickstart tested on a cold machine; optional quickstart scroll-depth ≥ 90%    |
| /docs → install conversion   | **> 35%** of docs visitors copy the install command                                                                            | Lightweight analytics event on install-copy / Install-page reach               |
| LLM / search discoverability | `llms.txt` + `llms-full.txt` + sitemap shipped; **≥ 5** target "how do I…" queries answered correctly by an LLM within 90 days | Search Console + manual prompts to ChatGPT/Claude/Perplexity                    |
| Reference freshness          | **0** doc-vs-code drift items per release; every config section has a runnable example                                          | Per-release checklist; reference pages generated via F2                        |

## Feature Assessment

| Criteria            | Question                                            | Score    |
| ------------------- | --------------------------------------------------- | -------- |
| **Impact**          | How much more valuable does this make the product?  | Strong   |
| **Reach**           | What % of users would this affect?                  | Must do  |
| **Frequency**       | How often would users encounter this value?         | Strong   |
| **Differentiation** | Does this set us apart or just match competitors?   | Strong   |
| **Defensibility**   | Is this easy to copy or does it compound over time? | Maybe    |
| **Feasibility**     | Can we actually build this?                         | Must do  |

Leverage type: **Strategic Bet** with a clear path to **Compounding** (the generated
corpus + `llms.txt` + a V2 in-product `/help` get more valuable over time).

## Council Insights

- **Recommended approach:** Hand-rolled multi-page `/docs` over the existing
  `global.css` (respecting the choice against a framework), with reference **generated
  from the Rust source** and only Quickstart + Governance hand-written. Ship
  **alongside** the README behind a CI link-check gate; keep the README canonical in V1.
- **Key trade-offs:** Generated reference (drift-proof, upfront generator work) vs
  hand-authored (no build step, guaranteed rot) → generated. Hand-rolled (brand
  cohesion, rebuild table-stakes) vs Starlight (free search/SEO/versioning, second
  visual language) → hand-rolled, with `llms.txt` covering the load-bearing SEO.
  Conversion-first vs reference-complete → both, since generated reference is nearly free
  once F2 exists.
- **Risks identified:** Base-path 404s that pass locally and fail under `/atelier` → CI
  link-check gate (F9). Generator scope creep → emit from existing surfaces, start with
  highest-churn tables. Slimming the README onto an unproven deploy → defer the flip to V2.
- **Stretch goal (V2+):** An in-product `/help` that RAGs the docs corpus (Aider's
  pattern) — docs become a runtime feature, not just a website. Plus the README→pointer
  flip, Starlight/search/versioning if needed, and a recipes catalog grown from real usage.

## Integration with Existing Features

| Integration Point          | How                                                                                  |
| -------------------------- | ------------------------------------------------------------------------------------ |
| Landing page (`index.astro`) | Add a base-aware "Docs" link to `nav__links`; hero CTA can point at Quickstart       |
| Design system (`global.css`) | Reuse `.section`, `.eyebrow`, `.command-table`, `.feature-card`, `.install__panel`; add a doc heading scale |
| README.md                  | Stays canonical in V1; its generated tables and the docs pages both project from F2   |
| Rust source surfaces       | `--print-config`, `--doctor --json`, `slash_commands.rs` catalog feed F2 at build time |
| GitHub Pages deploy        | New pages route under base `/atelier`; F9 link-check gates the existing Pages workflow |

## Out of Scope (V1)

- **README → pointer canonical flip** — deferred to V2. Slimming the never-404 GitHub
  storefront onto a first-ever Pages deploy risks broken first impressions; flip on a
  verified deploy + audience evidence, not one green build.
- **In-product `/help` RAG over docs** — V2. A new subsystem (embeddings, an index to
  keep in sync); the corpus must exist and be proven first.
- **Astro Starlight / docs framework** — declined in favor of F2 + F6 + `llms.txt`, which
  cover the load-bearing needs. Revisit when full-text search or versioning is actually
  required.
- **Broad recipes catalog** — V1 seeds 1-2 real recipes inline in the Quickstart; a
  standalone catalog grows from observed usage, not pre-written guesses.
- **MDX / syntax-highlighted code fences** — V1 uses the existing plain `<pre>` styling;
  revisit with a future MDX/Starlight decision.
- **Versioned docs, full-text search UI, i18n, architecture deep-dives, contributor
  guide** — premature at this stage.

## Architecture Decision Records

- [ADR-001: V1 docs — derive reference from the Rust source and ship alongside the
  README](adrs/adr-001.md) — Generate the reference, hand-write only Quickstart +
  Governance, ship alongside the README behind a link-check gate, defer the canonical
  flip and in-product RAG to V2.

## Open Questions

- **Generator mechanism:** a build-time Rust `xtask` emitting JSON consumed by Astro, vs
  invoking the installed `atelier --print-config` / `--doctor --json` at build? And does
  `--print-config` expose *every* section (e.g. `[presets.*]`, `[workspace.*]`, which the
  `--init-config` starter omits)?
- **Page granularity:** keep Commands & CLI as one page or split? Fold the
  agents/runtimes reference into Configuration, or give it its own page (a possible 6th)?
- **Analytics on static GitHub Pages:** what privacy-respecting measurement (if any)
  backs the conversion/TTFS KPIs — a lightweight script, or manual proxies only?
- **V2 canonical-flip trigger:** verified deploy alone, or a specific audience-demand
  threshold (the Devil's Advocate's condition)?
- **Governance page depth:** is a diagram of the action-gating flow (capability +
  approval + roots + limits → `ActionRequest`) worth the V1 effort, or text-only?
