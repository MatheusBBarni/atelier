# Idea: Session Transcript Export — Redacted, for Change Provenance

> Packet slug: `run-transcript-export`. V1 unit is the whole session (with run-scoping); the origin name "export-run" survives as the CLI verb — see Open Questions on final flag naming.

## Overview

`atelier` records every run as a durable, event-sourced JSONL log under `.atelier/sessions/<id>/`, but that history is **write-only in practice** — there's no way to fold a session into a portable artifact that can leave the machine. This feature adds a **non-interactive CLI export** (`atelier --export-session <id>`) that folds a session's log through the existing chat projection into a **redacted, human-readable Markdown transcript** — so a reviewer can see *what the agent did and why* attached to a PR.

It's for the **developer shipping AI-authored changes** (primary), the **reviewer/teammate** who reads the change, and the **security/compliance lead** who must trust nothing leaks. V1 is deliberately a **CLI-first wedge**: the read→project→sanitize path is ~80% reused from the shipped `session-browser-resume` feature, so the genuinely net-new work is the Markdown serializer, the secret-redaction pass, and a mandatory review-gate. V1 is explicitly **step 1 toward a compounding north star** — verifiable, signed AI-change provenance (an "AIBOM") — with a redacted **change digest** as the lead V2.

## Problem

`atelier` is an event-sourced system whose entire value rests on a durable action log, yet that log only ever projects *forward*, into a live TUI it just created. Once the session ends, the richest asset the system produces — the full record of which files were read and written, which commands ran, and why — is trapped as local JSONL. The only way to share "here's what the agent did" today is to hand-`cat` the log or paste screenshots: slow, lossy, and dangerous.

The provenance gap is now a *governance* problem, not just an ergonomic one. The industry norm for attributing AI work is a single `Co-Authored-By` commit trailer — itself contested — which says *that* an agent helped but nothing about *what it did*. Meanwhile auditors are beginning to require an AI-change audit trail under the EU AI Act and California AB 2013. atelier's event log already *is* that artifact; it just can't get out of the machine safely.

And "safely" is the crux. The moment a transcript leaves the machine into a PR — possibly a public repo — it crosses an **irreversible boundary**: a leaked credential in git history can only be rotated, never recalled. The transcript deliberately keeps diffs and command output, which is exactly where secrets hide. So the problem is two-sided: history is trapped, *and* the obvious way to free it is a one-command secret-exfiltration path if redaction isn't handled with discipline.

### Market Data

- **Redacted local export ships nowhere.** Aider, Gemini CLI, Goose, Codex CLI, and Claude Code all export (or persist) sessions with **zero** secret redaction; only Cursor redacts — and it's **cloud-only, paid-tier, and self-disclaimed** ("Redaction is not guaranteed and can miss secrets"). A local-first redacted file produced *before* anything leaves the machine is open whitespace.
- **The stakes are large and rising.** **23.8M** secrets leaked in public GitHub repos in 2024 (+25% YoY); **28.65M** in 2025 (+34%); **1.27M AI-service secrets** leaked in 2025 alone (+81%). Repos with an AI assistant active leak secrets **~40% more often**. **70%** of secrets leaked in 2022 are still valid. Average data-breach cost: **$4.4M** (IBM, 2025). *(GitGuardian/Snyk/GitHub/IBM.)*
- **Automated redaction provably leaks** — which is *why* a human gate is the defensible design, not friction. The best scanner (Gitleaks) has **88% recall (misses 12%)**; TruffleHog ~52%; **58% of real leaks are "generic" secrets** regex can't catch. No tool has both high precision and high recall.
- **Adoption is high, trust is low.** **84%** of developers use or plan to use AI coding tools, but trust **fell to 29%** (−11 pts YoY). A verifiable provenance artifact lands directly in that trust gap.

## Summary / Differentiator

Every competitor that exports does so with no redaction, or redacts only in their cloud on a paid tier while disclaiming it. atelier can ship the thing none of them do: a **local-first, redacted, human-reviewable provenance file** produced before a byte leaves the machine — and, because the redaction is provably imperfect, it turns the **mandatory human preview into a trust feature** ("you eyeball the redaction before the file exists") rather than hiding behind a false green check. Tied to atelier's event-sourced log, that artifact is the seed of a verifiable AI Bill of Materials that no competitor is positioned to match.

## Core Features

| # | Feature | Priority | Description |
|---|---|---|---|
| F1 | `--export-session` CLI command | Critical | Non-interactive entry point (sibling to `--doctor`/`--print-config`). Opens the session via `HistoryStore::open`, folds the log through `ChatProjection::rebuild`, serializes `ChatItemView`s to Markdown. Whole-session by default; `--run <id>`/`--last-run` to narrow scope. |
| F2 | Tiered secret redaction (detect, don't mutate) | Critical | Export-time scan over the rendered transcript. **Deterministic class** (credential-file bodies redacted wholesale; literal `AKIA…`/`ghp_…`/`xox…`/PEM/`Authorization:`/`*_TOKEN`/`*_SECRET`/`*_KEY=`/`password=`) is flagged + gated. **Fuzzy/entropy class** is advisory-only. Never rewrites kept diffs/commands/reasoning. |
| F3 | Mandatory redaction review gate | Critical | Before writing, present a **review surface**: flagged-secret count + highlighted matched spans (not a raw scroll). Output is never branded "safe"; footnoted best-effort, "rotate any credential the agent touched." |
| F4 | Tiered fail-closed under `--yes` | Critical | `--yes` (CI path) **fails closed** — non-zero exit, no file written — on any *deterministic* hit; override only via explicit, loudly-logged `--allow-flagged`. Fuzzy/entropy hits warn but don't block, so CI doesn't flake on SHAs/base64. |
| F5 | Quarantine egress | High | Write `0600` to a gitignored path outside the working tree by default (e.g. `.atelier/exports/`); warn if the target is inside a repo or isn't gitignored, so `git add -A` can't sweep a draft into history. |
| F6 | `session_exported` audit event | High | Append an event on export (timestamp, scope, redaction summary, target) — the provenance loop closing on itself, and the local signal for the safety/adoption KPIs. |
| F7 | Redaction recall gate (seeded corpus) | Medium | A committed recall bar measured against a seeded canary-secret corpus, run before any "safe to share publicly" guidance ships. Makes the safety claim empirical, not a vibe. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| Redaction safety | **0** confirmed secret leaks in shared exports | Incident reports + `session_exported` redaction-summary counts |
| Redaction recall (pre-launch gate) | **≥99%** recall on the *deterministic* class against the seeded corpus (human preview backstops the rest) | Automated test vs seeded secret shapes |
| Export adoption | **≥15%** of weekly-active users export ≥1 transcript within 30 days | `session_exported` events ÷ WAU (opt-in telemetry) |
| Human-review rate | **≥80%** of exports pass a reviewed preview (not `--yes`-blind) | Ratio of interactive vs `--yes`/`--allow-flagged` in the audit event |
| Time-to-shareable-file | **<60s** median (vs minutes hand-`cat`-ing JSONL) | Timed task / user report |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | **Strong** |
| **Reach** | What % of users would this affect? | **Maybe** |
| **Frequency** | How often would users encounter this value? | **Maybe** |
| **Differentiation** | Does this set us apart or just match competitors? | **Strong** |
| **Defensibility** | Easy to copy or compounds over time? | **Maybe** (→ Strong on the AIBOM trajectory) |
| **Feasibility** | Can we actually build this? | **Strong** (risk concentrated in redaction) |

Leverage type: **Strategic Bet** with a compounding trajectory toward verifiable provenance.

## Council Insights

- **Recommended approach:** Ship a **CLI-first, recall-gated, tiered-fail-closed** redacted session export, reusing the shipped fold pipeline. Defer the in-TUI `/export` to V2.
- **Key trade-offs:** CLI-first proves the Markdown format once before doubling the surface (the TUI half mutates the fixed slash-command catalog and adds a second confirm-gate). Redaction **detects but does not mutate** the narrative — except wholesale redaction of credential-file *bodies* — so the artifact stays reviewable. Whole-session default maximizes provenance but also blast radius; `--run` scoping is the mitigation.
- **Risks identified & mitigations:** (1) *False-negative leak* (primary) → deterministic fail-closed under `--yes`, file-level redaction, honest review-surface preview, quarantine path, seeded-corpus recall bar, never brand "safe." (2) *`--allow-flagged` becomes a copy-paste incantation* → loud logging, must be explicit. (3) *Low adoption / unread transcripts* → instrument attach-and-repeat with pre-committed kill/scale thresholds; the digest (V2) attacks the "nobody reads it" risk. **Preserved dissent (Devil's Advocate):** no public-repo adoption datapoint before a measured recall number exists.
- **Stretch goal (V2+):** Verifiable, hash-chained, signed provenance record (AIBOM) with a CI gate that checks every AI commit carries a valid record — the compounding, regulation-aligned endgame.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| `session-browser-resume` read path | Reuse `HistoryStore::open` + `read_events` and `build_session_preview` (open→rebuild→sanitize) — the safe, shipped 80% |
| `ChatProjection::rebuild` (`projection.rs`) | The fold the serializer walks; `ChatItemView`s → Markdown |
| `sanitize_transcript_text` | Existing ANSI/control stripping applied to exported text |
| CLI entry points (`src/cli.rs`) | `--export-session` mirrors `--doctor`/`--print-config` early-return, no TUI loop |
| Write-time redaction (`http_util.rs`) | Export-time pass is defense-in-depth on top of the existing (weak) write-time redaction |

## Sub-Features (V2+ trajectory)

- **Redacted change digest** — concise "files touched / commands / decisions / cost" summary for the PR body; the lead V2 (attacks "nobody reads a 2,000-line transcript"). Note: summarization may add LLM cost/nondeterminism — a cost consideration to weigh then.
- **In-TUI `/export`** — thin wrapper over the proven serializer + existing preview modal.
- **Verifiable AIBOM record** — structured, signed, hash-chained provenance + CI verification (the north star).

## Out of Scope (V1)

- **In-TUI `/export` command** — deferred to V2; prove the Markdown format on real PRs first, and avoid mutating the fixed slash-command catalog + doubling the confirm-path test surface before the format is validated.
- **Change digest / PR-body summary** — V2 (lead follow-on); adds summarization determinism/cost risk off V1's critical path.
- **Verifiable/signed AIBOM provenance record** — V2+ north star; a separate subsystem (format, signing, CI verification) of Massive scale.
- **Full entropy/Gitleaks-grade scanner as a hard gate** — rejected; a false-positive cannon over a diff-bearing transcript and a rotting ruleset. Entropy stays advisory-only.
- **Custom/configurable redaction patterns & cross-session/multi-session export** — deferred; V1 is one session with a fixed deterministic ruleset.
- **HTML/JSON output formats** — V1 is Markdown only (PR-attachable); structured output belongs to the AIBOM track.

## Architecture Decision Records

- [ADR-001: V1 scope — CLI-first redacted session export with tiered, detect-don't-mutate redaction](adrs/adr-001.md) — CLI-first (TUI to V2); whole-session + `--run` scoping; tiered redaction that flags but never mutates the narrative; tiered fail-closed under `--yes`; quarantine egress; recall-gated before "safe" guidance.

## Open Questions

- **Flag naming:** `--export-session` vs the origin `export-run` — atelier uses flags today, not subcommands; confirm the final name and whether `--run`/`--last-run` is the scoping interface.
- **Recall bar specifics:** the exact committed recall % and the seeded-corpus contents need security sign-off.
- **Deterministic ruleset membership:** which token shapes count as "high-confidence" (gating) vs advisory — a techspec/security decision.
- **Quarantine path:** default location (`.atelier/exports/` vs an explicit target) and whether to auto-append to `.gitignore`.
- **PR-attach measurement:** the primary-use signal (did the file reach a PR?) isn't observable from a local audit event — how do we learn attach-and-repeat without server-side telemetry?
- **Large-session legibility:** how to make flagged spans reviewable in a long transcript (jump-to-flag), including over stdout where there's no modal.
