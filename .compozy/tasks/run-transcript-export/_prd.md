# PRD: Session Transcript Export

> Packet: `run-transcript-export` · Source idea: `_idea.md` · Decisions: [ADR-001](adrs/adr-001.md), [ADR-002](adrs/adr-002.md)

## Overview

`atelier` records every run as a durable event log under `.atelier/sessions/<id>/`, but that history can't leave the machine — there's no way to turn a session into a shareable artifact. **Session Transcript Export** adds a non-interactive command, `atelier --export-session <id>`, that produces a **redacted, human-readable Markdown transcript** of a session so a developer can attach it to a pull request and show reviewers *what the agent did and why*.

It's for the **developer shipping AI-authored changes** (primary) and the **reviewer** who reads them; a **security-conscious lead** is the secondary beneficiary of the redaction guarantees. It's valuable because redacted local export ships nowhere else (competitors export with no redaction, or redact only in-cloud and disclaim it), and because a verifiable record of an agent's actions lands directly in the AI-trust gap (84% of developers use AI tools; trust has fallen to 29%). V1 is deliberately a lean, CLI-first wedge built on the ~80%-reused read→project→render path, pointed at a compounding verifiable-provenance north star.

## Goals

- Let a developer turn any session into a **redacted, PR-ready Markdown file in under 60 seconds**, replacing the error-prone hand-`cat`-the-JSONL workaround.
- Make secret leakage through an exported transcript a **non-event**: zero confirmed leaks, enforced by a review gate that fails closed in automation.
- Produce a transcript a reviewer **actually reads** — summary-first, mapping the agent's intent to the change — rather than an ignored dump.
- Ship the smallest validated surface (CLI) and earn the next surface (TUI) with usage data: instrument **attach-and-repeat** against pre-committed kill/scale thresholds.
- Milestone: V1 is the CLI command + redaction + risk-adaptive gate; the in-TUI `/export` and the change digest follow in Phase 2.

## User Stories

**Primary — PR author**
- As a developer who just had atelier make a multi-step change, I want to export a redacted transcript of the session to a file, so I can attach it to my PR and show reviewers what the agent did and why — without leaking secrets.
- As a developer with a commit hook or CI step, I want a non-interactive export that **fails closed** when it can't confidently redact, so provenance is captured automatically and a secret never rides along into a public PR.
- As a developer scoping a noisy session, I want to export just the relevant run instead of the whole session, so the transcript stays focused.

**Primary — Reviewer**
- As a reviewer, I want a summary-first transcript with the agent's execution trail and a clear redaction summary, so I can confirm the change matches its description without reading thousands of lines.

**Secondary — Security-conscious lead**
- As a security-conscious lead, I want every export to pass a redaction review and every override to be logged, so sharing agent transcripts never becomes an unaudited leak path.
- As a reviewer of a flagged export, I want to dismiss an obvious false-positive flag for this export, so a base64 hash mistaken for a secret doesn't block me.

## Core Features

| # | Feature | Priority | What it does |
|---|---|---|---|
| CF1 | Session export command | Critical | `atelier --export-session <id>` renders a session's log to a redacted Markdown transcript; whole-session by default, with run-scoping to narrow it. Writes to a file or stdout. |
| CF2 | Precision-first secret redaction | Critical | High-confidence secrets (credential files, recognizable keys/tokens, `Authorization`/`password=`) are flagged and redacted by explicit label; low-confidence/entropy matches are advisory only. Redaction never rewrites the change narrative (diffs/commands/reasoning) — except a credential file's body. |
| CF3 | Risk-adaptive review gate | Critical | Before writing, a clean export confirms with one key; a flagged export requires acknowledging the flagged count and categories. The gate escalates friction only when there's a real decision. |
| CF4 | Safe-by-default automation mode | Critical | `--yes` runs non-interactively and **fails closed** (non-zero exit, no file) on any high-confidence hit; override only via an explicit, logged `--allow-flagged`. Makes CI provenance safe by construction. |
| CF5 | Lean review-artifact format | High | A TL;DR + redaction summary lead the file; verbose file-reads and tool output collapse; flagged spans are surfaced. Optimized for a reviewer who skims and a checker who needs the dangerous parts to stand out. |
| CF6 | Quarantined output + audit trail | High | The file lands outside the working tree with owner-only permissions; a warning fires if the target is git-tracked or not ignored. The export and any overrides are recorded to the session log. |
| CF7 | Per-export false-positive dismissal | Medium | The user can mark a flagged span "not a secret" for the current export, keeping the gate usable without a persistent allowlist. |

## User Experience

**Discovery.** The flag appears in `atelier --help` alongside `--doctor`/`--print-config`, with a one-line description matching the house tone. `/help` notes that the in-TUI export is coming in a later phase.

**Primary flow — interactive, clean session:**
1. Developer finishes a session and runs `atelier --export-session <id> --out provenance.md` (or omits `<id>` to take the latest session).
2. The tool renders the lean transcript and finds no high-confidence secrets.
3. It shows a one-line summary ("3 files changed, 2 commands, 0 secrets flagged") and a single-key confirm.
4. The developer confirms; the file is written outside the repo tree with a path + "review before sharing; redaction is best-effort, rotate any credential the agent touched" notice.
5. The developer moves/links the file into the PR.

**Primary flow — flagged session:**
1. Same start; redaction flags 1 high-confidence secret (an env value in a diff).
2. The gate shows the flagged count and categories and requires an explicit acknowledgment (atelier's `approve`/`n` vocabulary) — not a bare keypress.
3. If a flag is an obvious false positive, the developer dismisses it for this export; a genuine secret stays redacted by label.
4. On acknowledgment the file is written and the override (if any) is recorded.

**Automation flow — CI / commit hook:**
1. A hook runs `atelier --export-session <id> --yes --out provenance.md`.
2. With no high-confidence hit, the file is written and the step passes.
3. With a high-confidence hit, the command **fails closed**: non-zero exit, no file, a message in the `--strict`-style "re-run with --allow-flagged to override" idiom. The leak never reaches the PR.

**UX considerations.** The gate reuses approval-modal vocabulary (`y`/`n`/explicit token), redaction reuses the `<redacted>`-style label convention (the fact of redaction is always visible, never silently hidden), and CLI output follows the established exit-code and stderr-error conventions. Flagged spans are the only thing expanded by default; everything verbose collapses.

## High-Level Technical Constraints

- **Builds on the event-sourced log and chat projection** — the transcript is a rendering of recorded events, not a new data source.
- **Redaction precedes any write**, and the export must never weaken or bypass the existing write-time redaction; secrets are never written unredacted.
- **Respects the workspace's read/write roots and uses owner-only file permissions**; the default output location is outside the working tree.
- **The artifact is explicitly best-effort, not a certified/safe guarantee** — V1 must not market output as "safe to publish"; a measured redaction-recall bar on a seeded secret corpus gates that claim.
- **Performance:** exporting a long session should complete in a few seconds with no perceptible stall, since it's an interactive command.

## Non-Goals (Out of Scope)

- **In-TUI `/export` command** — deferred to Phase 2; CLI-first proves the format where the provenance job lives.
- **Change digest / PR-body summary** — Phase 2; the lead follow-on, but it adds summarization risk off V1's path.
- **Verifiable/signed AIBOM provenance record + CI verification** — Phase 3 north star; a separate subsystem.
- **Persistent "not-a-secret" baseline & custom/configurable redaction patterns** — later; V1 dismisses false positives per-export only.
- **Cross-session / multi-session export, transcript search** — V1 is one session.
- **HTML or JSON output formats** — V1 is Markdown only; structured output belongs to the AIBOM track.
- **Auto-attaching the transcript to a PR** — the developer places the file; atelier produces it.
- **A full entropy/Gitleaks-grade scanner as a hard gate** — rejected; the fuzzy class stays advisory to protect precision.

## Phased Rollout Plan

### MVP (Phase 1) — CLI export
- CF1–CF7: the export command, precision-first redaction, risk-adaptive gate, fail-closed automation mode, lean format, quarantined output + audit trail, per-export FP dismissal.
- **Success criteria to proceed:** the committed redaction-recall bar is met on the seeded corpus; zero confirmed leaks in dogfooding; an early attach-and-repeat signal (the same users export more than once).

### Phase 2 — In-flow + readable
- In-TUI `/export` over the proven serializer + preview modal; the redacted **change digest** for the PR body; a persistent false-positive baseline.
- **Success criteria to proceed:** attach-and-repeat clears the scale threshold; reviewers report the digest is what they read.

### Phase 3 — Verifiable provenance
- Structured, signed, hash-chained AIBOM record + an optional CI gate that checks every AI commit carries a valid record.
- **Long-term success:** adopted by teams under audit mandates as their AI-change record.

## Success Metrics

| Metric | Target |
|---|---|
| Confirmed secret leaks in shared exports | **0** |
| Redaction recall on the deterministic class (seeded corpus) | **≥99%** before any "safe to publish" guidance |
| Weekly-active users who export ≥1 transcript within 30 days | **≥15%** |
| Exports passing a human-reviewed gate (not `--yes`-blind) | **≥80%** |
| Median time from intent to shareable file | **<60s** |
| Attach-and-repeat (same user exports again within 30 days) | tracked, with pre-committed kill/scale thresholds |

## Risks and Mitigations

- **Low adoption — transcripts generated but unread/unused.** Mitigation: instrument attach-and-repeat with kill/scale thresholds; cheap reuse caps the sunk cost; the Phase-2 digest targets the "nobody reads it" failure directly.
- **Rubber-stamping the review gate.** Mitigation: risk-adaptive friction (active acknowledgment only when flagged), logged categorized overrides, summary-first artifact.
- **False-positive fatigue driving blanket bypass.** Mitigation: precision-first flagging; the entropy class is advisory, never blocks; per-export dismissal.
- **Reputation risk if a leak slips through a "redacted" file.** Mitigation: fail-closed automation, never brand output "safe," the recall-bar gate, owner-only quarantined output, "rotate touched credentials" notice.
- **Competitive risk.** Low — redacted local export is open whitespace; the main race is the verifiable-provenance trajectory, addressed by the phased plan.

## Architecture Decision Records

- [ADR-001: V1 scope — CLI-first redacted session export with tiered, detect-don't-mutate redaction](adrs/adr-001.md) — the scope and redaction-security architecture (from the idea phase).
- [ADR-002: V1 product experience — CLI-first export, lean review artifact, risk-adaptive review gate](adrs/adr-002.md) — the delivery surface, artifact shape, confirmation experience, and override/false-positive handling.

## Open Questions

- **Flag naming:** confirm `--export-session` vs the origin `export-run`, and the run-scoping flag's name (`--run`/`--last-run`).
- **Recall bar specifics:** the exact committed recall % and the seeded-corpus contents need security sign-off.
- **Override categories:** which categories the audit log records for a bypass (false-positive / intentional / fix-later, per the push-protection model).
- **Acknowledgment ergonomics:** whether the flagged-export acknowledgment is a keystroke, a typed count, or a typed token — a UX detail for the TechSpec.
- **PR-attach measurement:** the primary-use signal (did the file reach a PR?) isn't observable from a local audit event — how to learn attach-and-repeat without server-side telemetry.
- **Large-session legibility over stdout:** how flagged spans stay reviewable when piped (no modal, no scroll).
