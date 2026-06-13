# PRD — Atelier Documentation Site (`/docs`)

## Overview

Atelier's marketing landing page convinces a developer, then dead-ends — there is no
path from "I'm interested" to a running, configured tool. The only documentation is a
single ~296-line GitHub README that mixes install, CLI, config, agents, runtimes, and
layout; an evaluator must read all of it, and a configurer must scroll it to find one
fact. Worse, the reference content the site *does* carry already drifts from the code,
and a developer who asks an LLM how to use Atelier gets a confident guess.

This PRD defines a small, multi-page `/docs` section on the existing Astro site that
takes a **first-time evaluator** from install to a successful orchestrated run in
minutes, gives a **hands-on configurer** an accurate reference for `multiagent.toml` and
the commands, and serves the **LLMs** developers now ask first with a machine-readable
corpus. It is positioned as a **differentiation-led activation surface**: it leads with
the control-plane / safety story no competitor documents, keeps reference **generated
from the Rust source** so it cannot drift, and ships **alongside** the README (which
stays canonical for now).

## Goals

- **Activate evaluators:** a Quickstart that reliably reaches a real first run, with a
  sub-5-minute design target.
- **Serve configurers:** complete, accurate, scannable reference for every
  `multiagent.toml` section, CLI flag, and slash command.
- **Win LLM-mediated discovery:** ship `llms.txt` + `llms-full.txt` + per-page Markdown
  twins so assistants cite Atelier correctly instead of hallucinating.
- **Differentiate:** a flagship Governance & Safety page and a Concepts page that make
  Atelier's control-plane model the reason to adopt.
- **Kill drift:** 0 doc-vs-source drift at launch and per release, enforced by generated
  reference.
- **Milestones:** **Wave 1** (hand-written pages + foundation + `llms.txt`) ships first;
  **Wave 2** (generated reference) follows within V1. The README→pointer flip and
  in-product `/help` are explicitly V2.

## User Stories

**Evaluator Eli (first-time, in "study" mode)**

- As a developer who just found Atelier, I want to see the orchestration loop work in
  under a minute *without* setting up credentials, so that I can judge whether it's worth
  a real setup.
- As an evaluator of an *agentic* tool, I want to understand exactly what an agent can
  and cannot touch on my machine, so that I can trust pointing it at my repo.
- As a newcomer, I want one Quickstart that ends in a real run, so that I reach a "first
  win" before deciding.

**Configurer Cora (returning, in "work" mode)**

- As an active user, I want to look up the exact key, default, and fallback behavior for
  any `multiagent.toml` setting, so that I can tune agents, runtimes, models, and limits
  with confidence.
- As a configurer, I want a complete command/flag reference that matches the installed
  binary, so that I never act on stale docs.

**Ada the Assistant (non-human reader)**

- As an LLM a developer asked "how do I add a Z.ai runtime in atelier?", I want an
  authoritative, machine-readable corpus, so that I answer correctly instead of guessing.

**Maintainer**

- As the maintainer, I want reference content generated from the source of truth, so that
  documentation stays correct as the code moves without manual upkeep.

## Core Features

Grouped by priority; "Wave" marks V1 delivery sequence.

| #   | Feature                              | Priority | Wave | What it does                                                                                                                                                                                                                                                              |
| --- | ------------------------------------ | -------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1  | **Quickstart (lazy + fake-first)**   | Critical | 1    | Install → `atelier --doctor` → an optional zero-setup `fake` preview of the loop → connect one real runtime/key → a read-only first run → a first **approved write** that surfaces the approval prompt as the safety "aha" → next steps. The activation north star.        |
| F2  | **Governance & Safety page**         | Critical | 1    | Hand-written, source-anchored: what an agent can and can't touch (mediated actions, read vs write roots), the two layers (capabilities = what's *allowed*, approval mode = when you're *asked*), limits, and the durable replayable record. The unclaimed differentiator. |
| F3  | **Concepts — how Atelier works**     | High     | 1    | The mental model: the orchestrator run loop, run lifecycle, agents vs runtimes, the control plane. Lets the evaluator understand (and trust) before adopting.                                                                                                            |
| F4  | **Shared docs foundation**           | High     | 1    | A `Base.astro` layout (head, fonts, nav, footer), docs nav + sticky TOC, base-aware (`/atelier`) cross-page links, a doc-sized heading scale, and a "Docs" entry in the landing-page nav.                                                                                |
| F5  | **Machine-readability assets**       | High     | 1→2  | `llms.txt` + sitemap in Wave 1; `llms-full.txt` + per-page Markdown twins in Wave 2. Question-shaped headings throughout.                                                                                                                                                |
| F6  | **Source-derived reference generator** | Critical | 2  | Emits reference content from the existing Rust surfaces (`--print-config`, `--doctor --json`, the slash-command catalog) so reference pages can't drift. Ends the drift bug class.                                                                                        |
| F7  | **Configuration reference**          | High     | 2    | Every `multiagent.toml` section — agents, runtimes, council, limits, ui, workspace — generated via F6. (`[presets.*]` hand-written from the `--init-config` template, since it isn't in the merged-config output.)                                                       |
| F8  | **CLI & Commands reference**         | High     | 2    | All CLI flags + their validation rules, the slash-command catalog, and TUI keys, generated via F6. Troubleshooting checks (from `--doctor`) surface here and in the Quickstart.                                                                                          |
| F9  | **CI link-check gate**               | Medium   | 1    | A new PR-triggered `web/` checks workflow that builds the site and fails on broken links, so base-path `/atelier` 404s never reach users.                                                                                                                                |
| F10 | **README accuracy fix**              | Medium   | 1    | Correct the README "Requirements" wording that labels all runtimes "Optional" (the orchestrator effectively requires `ZAI_API_KEY`), so first-run expectations are honest.                                                                                               |

## User Experience

**Eli's flow (Quickstart):** lands on `/docs` → Quickstart → installs → runs the `fake`
preview and watches the orchestrator route a step with zero setup → connects a runtime he
already uses (or a key) → runs a read-only prompt ("summarize this project") and sees the
agent request only read actions → runs a prompt that writes a file and meets the
**approval prompt** — the moment the control plane becomes the selling point, not a wall →
follows links into Concepts and Governance.

**Cora's flow (reference):** arrives via search/nav directly at Configuration or
CLI & Commands → finds the exact key/flag, its default, and behavior → trusts it because
it's generated from the binary she's running.

**Ada's flow (LLM):** fetches `/llms.txt`, follows it to a page's Markdown twin (or
`/llms-full.txt`), and returns an accurate answer with a correct flag.

**Discoverability & access:** a "Docs" link in the landing nav and hero; a sitemap and
`llms.txt` for crawlers and assistants; question-shaped headings and stable anchors. The
pages reuse the existing terminal-aesthetic design system (semantic color tokens, the
`.command-table` reference primitive), add a readable doc heading scale, and remain a
fast, keyboard-navigable static site with no user tracking.

## High-Level Technical Constraints

- Must ship as a **static site under the `/atelier` base path**; every cross-page link
  and asset must be base-aware (pass locally *and* on GitHub Pages).
- Reference content must **derive from the Rust source of truth**; it must not be a
  hand-maintained copy.
- Must **reuse the existing visual design system** (no second framework's look); the
  bespoke terminal aesthetic is a feature.
- **No user tracking / analytics** in V1 (privacy-respecting, instrument-free measurement
  only).
- Must **not regress** the existing landing page or its deploy.

## Non-Goals (Out of Scope)

- **README → pointer canonical flip** — V2, earned by a verified production deploy +
  audience evidence. The README stays canonical and never-404 in V1.
- **In-product `/help` that RAGs the docs** — V2; the corpus this PRD builds is its
  prerequisite.
- **A docs framework (Astro Starlight) / built-in full-text search / versioned docs /
  i18n** — not in V1; revisit if search or versioning becomes necessary.
- **A broad recipes catalog** — V1 seeds 1–2 real recipes inline in the Quickstart; a
  catalog grows from observed usage.
- **MDX / syntax-highlighted code fences** — V1 uses the existing plain code styling.
- **Architecture deep-dive & contributor guide** beyond the Concepts page — later.
- **Conversion / time-to-first-success tracking** — design goals in V1, not instrumented
  metrics.

## Phased Rollout Plan

### MVP — Wave 1 (V1)

- F1 Quickstart, F2 Governance & Safety, F3 Concepts, F4 shared foundation + landing nav
  link, F5 (`llms.txt` + sitemap), F9 link-check gate, F10 README fix.
- **Success criteria to proceed:** the site deploys and resolves under `/atelier` with
  the link-check green; the three hand-written pages pass an accuracy review; the
  Quickstart is validated on a cold machine to reach a real run; `llms.txt` validates and
  a baseline LLM-citation spot-check is recorded.

### Wave 2 (still V1)

- F6 generator, F7 Configuration reference, F8 CLI & Commands reference, F5 completion
  (`llms-full.txt` + Markdown twins), folded Troubleshooting.
- **Success criteria:** 100% reference coverage vs the Rust source; **0 drift**; reference
  regenerates as part of the build.

### Phase 3 (V2+)

- README→pointer canonical flip; in-product `/help` RAG; recipes catalog;
  search/versioning if demand appears.
- **Long-term success:** docs become the primary, canonical surface with the in-product
  help loop closing on the same corpus.

## Success Metrics

Instrument-free (no user tracking):

- **Reference coverage:** 100% of CLI flags, slash commands, and `multiagent.toml`
  sections documented at Wave 2 launch.
- **Drift:** **0** doc-vs-source drift items at launch and per release (generated
  reference + per-release checklist).
- **Machine-readability:** `llms.txt` + `llms-full.txt` + sitemap shipped and valid;
  per-page Markdown twins present.
- **LLM citation:** ≥ 5 target "how do I… in atelier" queries answered correctly by
  ChatGPT/Claude/Perplexity within 90 days (manual spot-check).
- **Reliability:** link-check passes on every deploy; the Quickstart is re-validated to
  reach a real run each release.
- *Design goals (not tracked in V1):* docs→install conversion; median
  time-to-first-successful-run < 5 min.

## Risks and Mitigations

- **No audience yet / nobody reads docs.** → Ship `llms.txt` so assistants surface
  Atelier; link docs from the landing nav and hero; keep the README canonical so
  GitHub-first visitors lose nothing.
- **Fake-first reads as "the tool needs fakery to demo."** → Frame `fake` explicitly as a
  zero-setup *preview* and move to a real run within the same Quickstart.
- **Governance page over-promises safety.** → Keep every claim source-anchored; include an
  honest-limits note ("strong boundaries, not a guarantee").
- **Looks unlike the category (no Mintlify polish).** → Treat the bespoke terminal
  aesthetic as a deliberate signal of a tool that owns its stack.
- **Wave 2 depends on a new generator pattern + small Rust enablers.** → Wave 1 ships
  independently of it; the generator reuses existing `--print-config` / `--doctor`
  surfaces and needs only a one-line enabler for the slash catalog.
- **Reference drift returns over time.** → Generated reference + a per-release coverage
  checklist.
- **First-run expectations set wrong by the README "Optional" wording.** → F10 corrects it
  in Wave 1.

## Architecture Decision Records

- [ADR-001: V1 docs — derive reference from the Rust source and ship alongside the
  README](adrs/adr-001.md) — Generate reference, hand-write only the prose pages, ship
  alongside the README, defer the canonical flip and RAG to V2.
- [ADR-002: V1 docs product approach — differentiation-led activation
  surface](adrs/adr-002.md) — Lead with Governance + Concepts + a lazy/fake-first
  Quickstart + first-class `llms.txt`; two waves; instrument-free metrics.

## Open Questions

- **`[presets.*]` reference:** hand-write from the `--init-config` template, or extend the
  config output so presets are generatable too? (Resolve in TechSpec.)
- **Slash-catalog enabler:** is touching the Rust source to expose the command catalog
  (for generation) acceptable, or should the CLI/commands reference stay hand-written in
  V1? (Resolve in TechSpec.)
- **Markdown twins:** auto-generated per page or authored — and the exact `llms-full.txt`
  assembly. (Resolve in TechSpec.)
- **Governance page depth:** include a visual of the action-gating flow (capability +
  approval + roots + limits → action), or text-only?
- **V2 canonical-flip trigger:** verified deploy alone, or a specific audience-demand
  threshold?
