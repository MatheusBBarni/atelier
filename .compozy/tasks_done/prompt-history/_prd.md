# PRD: Prompt History — Per-Project ↑/↓ Recall

## Overview

`atelier` routes every prompt through the orchestrator from one TUI input box
that has no memory — every prompt is typed from scratch. **Prompt History** adds
shell-style recall: press ↑/↓ to walk through prompts you previously submitted
*in this project*, persisted across restarts. It serves developers who re-run
failed steps, tweak long multi-clause goals, and return to a project after time
away. It is valuable because the input box is the most-used surface in the tool,
↑-recall is a universal terminal reflex, and the prompts are *already recorded* —
so recall removes the highest-frequency friction at minimal cost while laying the
substrate for an outcome-aware differentiator.

## Goals

- **Eliminate retyping** for the two dominant moments — re-run-after-failure and
  tweak-a-goal. *(Recall adoption > 20% of submissions.)*
- **Match table-stakes parity** users expect from shell history (newest-first
  ↑/↓, dedup, draft-preservation). *(0 multi-line collision defects; drafts never lost.)*
- **Own the per-project default** that Codex leaves unshipped — recall surfaces
  this project's prompts, not a global mix.
- **Ship a cohesive, correct V1 in one release** without regressing the input
  keypath. *(Recall step < 16ms; history available < 300ms post-launch, non-blocking.)*
- **Lay the V2 substrate** for outcome-aware recall with no rework.

*Timeline: single release for Phase 1.*

## User Stories

**Primary — the iterating developer**

- As a developer whose step just failed, I want to press ↑ and get my exact
  previous goal back, so I can resubmit or tweak one clause without retyping 200
  characters.
- As a developer refining a goal, I want to recall my last few prompts and edit
  one before submitting.

**Secondary — the returning developer**

- As a developer returning to a repo after days, I want to ↑ through recent
  prompts, so I can remember what I was driving the harness to do.

**Tertiary — the muscle-memory power user**

- As a terminal power user, I want ↑ to recall my last input the instant I open
  the app, because every shell I use behaves that way.

**Privacy-conscious**

- As a user who sometimes pastes sensitive text, I want a prompt I prefix with a
  space left out of recall, and a setting to turn recall off entirely.

**Edge**

- As a user editing a wrapped multi-line draft, I want ↑/↓ to move my cursor
  through the draft — not jump to history and lose my work.

## Core Features

*Critical = correctness/core · High = parity · Medium = convenience/governance*

- **↑/↓ recall (Critical):** With the cursor at the top/bottom edge of the input
  and no dropdown or queue focus, ↑ recalls the previous submitted prompt and ↓
  the next, newest-first; the recalled text replaces the input. Recall includes
  all submitted inputs (natural-language prompts *and* slash commands).
- **Collision-safe gating (Critical):** When the draft spans multiple visual
  rows, ↑/↓ move the cursor within the draft; history only steps at the top/bottom
  boundary. Avoids the #1 competitor bug (history eating a multi-line draft).
- **Draft preservation (Critical):** An in-progress draft is preserved when
  browsing begins and restored when ↓ steps past the newest entry. Recall never
  loses typed work.
- **Per-project scope (High):** Recall surfaces prompts from this project's
  workspace only.
- **On-by-default + disable toggle (High):** Recall works out of the box; a
  configuration setting disables it.
- **Consecutive-dedup + size cap (High):** Identical back-to-back prompts collapse
  to one entry; the recall list is bounded by a configurable maximum (default 200).
- **Leading-space-skip (Medium):** A prompt beginning with a space is omitted from
  recall — the familiar shell escape hatch for sensitive input.
- **Discoverability — hint line + help (Medium):** When recall is available, a
  subtle hint (e.g., "↑ recall") appears in the existing contextual hint line; the
  `/help` overlay documents the keys.
- **Recall instrumentation (Medium):** Each submission is tagged recalled-vs-fresh
  and recall depth is logged, so the future search decision is data-driven.

*Feature interactions:* recall yields to the dropdowns
(agent/skill/file-mention/command), the clarification prompt, and queue
navigation, which already own ↑/↓ in their contexts; recall is active only when
none of those claim the keys. The hint line reflects whichever mode currently
owns ↑/↓.

## User Experience

**Primary flow (re-run):**

1. A step fails; the user starts from an empty input.
2. ↑ → the most recent prompt fills the input; the hint line shows the recall affordance.
3. ↑ again → the prior prompt; ↓ → back toward newest; past newest → their original draft returns.
4. The user edits a clause and presses Enter → the edited prompt submits, tagged as recalled.

**Multi-line flow (no collision):**

1. The user is composing a wrapped, multi-row draft.
2. ↑ moves the cursor up a visual row; only at the top row does a further ↑ step into history.

**Discoverability/onboarding:** no separate onboarding — the feature matches
universal muscle memory. Discovery is reinforced by the contextual hint line when
recall is available and an entry in `/help`. **Empty-state** (fresh project, no
history): the first ↑ does nothing visible; a future cross-project scope path will
be taught here (deferred).

**Accessibility:** keyboard-only, consistent with the rest of the TUI; no
color-only signaling (hint text is literal); honors `NO_COLOR` via existing theme
tokens.

## High-Level Technical Constraints

- **No new at-rest data:** recall surfaces prompts already recorded for the
  project; it must not create a new durable store of prompt text. *(The underlying
  record is already access-restricted and excluded from version control.)*
- **Performance (user-perceived):** recall available shortly after launch without
  blocking first paint (target < 300ms for typical histories); each ↑/↓ step feels
  instant (< 16ms).
- **Integration boundary:** recall reads the existing per-project prompt record;
  it must not alter how prompts are recorded or how runs execute.
- **Privacy/security:** provide a leading-space exclusion and a global disable; do
  not weaken existing on-disk protections.
- **Keypath safety:** must not regress existing ↑/↓ behaviors (cursor movement,
  queue navigation, dropdown selection, clarification).

## Non-Goals (Out of Scope)

- **Reverse-search / "find a prompt" (Ctrl-R):** deferred behind a measured
  trigger; also requires resolving a keybinding conflict (Ctrl-R already resumes
  the queue).
- **Browsable history panel/archive:** a separate navigable screen; not in this effort.
- **Outcome-aware recall** (files-changed/cost/interrupted annotations): the
  committed V2 differentiator, designed separately.
- **Cross-project / global recall and scope-cycling:** fast-follow for the
  fresh-project empty state.
- **Secret redaction of stored prompts:** a persistence-layer concern; the prompt
  is already stored unredacted today, so recall adds no new exposure.
- **Editing or deleting individual history entries:** not in V1.

## Phased Rollout Plan

### MVP (Phase 1) — this PRD

Full faithful-parity recall in one release: ↑/↓ recall, collision-safe gating,
draft preservation, per-project scope, on-by-default + toggle, dedup + size cap,
leading-space-skip, hint-line + help discoverability, recall-everything,
instrumentation.

**Proceed when:** 0 multi-line collision defects in dogfooding; recall adoption
trending > 20%; no input-keypath regressions; latency targets met.

### Phase 2 — Outcome-aware recall

Annotate recalled entries with their outcome (files changed, cost,
interrupted/limit-reached, model) from the originating run.

**Proceed when:** users report outcome context changes which prompt they pick;
sustained recall adoption.

### Phase 3 — Search & scope

Reverse-search over the recall list (resolving the Ctrl-R conflict) and
cross-project scope-cycling (this-project → all-projects) with an empty-state
affordance.

**Long-term success:** recall is the primary path for repeat prompts; search
adoption justifies its surface.

## Success Metrics

- **Recall adoption:** > 20% of submitted prompts originate from a recalled entry.
- **Repeat-prompt recall:** > 30% of exact-repeat prompts come via recall vs retyped.
- **Correctness:** 0 multi-line collision defects; 0 reports of lost drafts.
- **Responsiveness:** history available < 300ms post-launch (non-blocking); ↑/↓
  step < 16ms.
- **Opt-out rate:** < 5% of users disable recall (confirms on-by-default is right).

## Risks and Mitigations

- **Adoption — users don't discover it:** contextual hint line + `/help` entry;
  also matches universal muscle memory.
- **Competitive — below the search bar:** ↑/↓-only trails competitors' Ctrl-R; own
  the per-project default now and commit outcome-aware recall (a gap no competitor
  fills) as the next differentiator.
- **Privacy perception — "it's saving my prompts":** message that recall surfaces
  what's *already* recorded (no new store), plus leading-space-skip and a disable toggle.
- **UX — the multi-line collision:** the #1 competitor failure; boundary gate +
  dedicated test matrix; frame V1 as "recall recent prompts," not "searchable
  history," to set expectations.
- **Scope — V2 creep:** keep outcome-metadata and search firmly in later phases;
  the projection design means deferring costs no rework.
- **Empty-state — fresh project feels broken:** the hint only shows when history
  exists; fully addressed by Phase 3 scope-cycling.

## Architecture Decision Records

- [ADR-001: V1 Prompt History as Per-Project ↑/↓ Recall Projected from the Event Log](adrs/adr-001.md)
  — recall is a read-only projection over the existing event log (not a new file);
  per-project default; newest-first; collision-gated; draft-preserving; leading-space-skip.
- [ADR-002: V1 Ships the Full Faithful-Parity Recall Set in One Release](adrs/adr-002.md)
  — deliver the complete shell-parity set at once (Approach A) rather than phasing
  the conventions (B) or pulling an outcome marker forward (C).

## Open Questions

- **Search widen-trigger threshold:** what measured value (recall reuse-rate or
  median recall-depth) flips the decision to build reverse-search in Phase 3? Needs a number.
- **Cross-session ordering:** strict timestamp interleave vs session-grouped,
  newest-first. *(Strict timestamp recommended for shell-fidelity.)*
- **Disable-toggle semantics:** does disabling stop recall only, or also stop the
  recalled/fresh instrumentation tagging?
- **`/clear`-style boundary:** if a conversation-clear affordance is ever added, it
  must not wipe input recall (a known competitor regression); confirm the decoupling.
- **Empty-state copy:** exact hint shown on the first empty ↑ in a fresh project.
