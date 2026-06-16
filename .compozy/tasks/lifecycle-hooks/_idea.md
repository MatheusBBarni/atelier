# Lifecycle Hooks — A Cross-Runtime Observer & Audit Layer Over the Event Spine

## Overview

`atelier` records every step of every run as a durable event, yet nothing outside the process can react to those events. There is no way to be pinged when a run pauses for approval, no way to ship an audit trail to an external system, no way to run a lint on each edit. This idea adds a `[hooks.<event>]` configuration that maps lifecycle events to user shell commands, each receiving the event payload as JSON on stdin.

V1 ships the **fire-and-forget observer tier only**, but re-anchored on the one job no competitor can copy: **uniform cross-runtime audit**. One hook config emits a normalized, versioned payload *identically* across Codex, Claude, Cursor, and zai — turning atelier's event spine into a single control plane over heterogeneous agent runtimes. A minimal notification slot and a stable `atelier events --follow` stream ride along. The blocking "veto" tier (policy-as-code) is deliberately deferred to V2, which reuses the same payload schema and dispatcher.

This is a **Strategic Bet**: V1 is the first, low-risk increment of a governance/integration platform whose moat is the cross-runtime control plane — not the hook mechanism itself, which is now industry-standard.

## Problem

atelier is event-sourced — `record_event` funnels every lifecycle moment into per-session `.atelier/` history, and the chat UI is merely a projection of that stream. But the event spine is *internal*. A developer who starts a long run and walks away has no way to learn that it has been sitting in `WaitingForUser` for twenty minutes awaiting an approval. A platform engineer who wants every agent action shipped to an internal observability system has to scrape JSONL after the fact. A team that wants lint-on-edit, or an audit trail for compliance, has no supported extension point at all. The only policy gate today is the built-in command classifier plus `ApprovalMode` — there is no way to react to, or record externally, what the harness does.

The deeper gap is **governance across runtimes**. atelier's whole premise is routing work through *pluggable* runtimes (Codex, Claude, Cursor, zai). An organization standardizing on atelier wants one audit trail and one policy posture over *all* of them. Today that is impossible: each underlying agent CLI has its own hooks, locked to itself, producing incompatible event shapes. The value atelier can uniquely offer — one config, one normalized audit trail, every runtime — does not exist yet.

This matters now because adoption is outrunning controls. Enterprises are deploying agentic tooling faster than they can govern it, and the missing capability they name most is precisely an auditable record of agent actions.

### Market Data

- **Only 21% of organizations have a mature governance model for agentic AI** (Deloitte, *State of AI in the Enterprise*, Apr 2026; 3,235 leaders, 24 countries). The single most-cited missing capability is **"audit trails that capture the full chain of agent actions."**
- **74% expect moderate-to-extensive AI-agent usage within ~2 years**, up from 23% today (same survey) — adoption is explicitly outrunning guardrails.
- **>40% of agentic-AI projects will be canceled by end of 2027**, partly from **inadequate risk controls** (Gartner, Jun 2025).
- **Competitive reality:** Claude Code, Cursor (1.7+), and Codex CLI have all already shipped this exact hook mechanism (events → shell → JSON on stdin → block via exit-2/decision-JSON) plus desktop notifications. The mechanism is **table stakes**; each competitor's hooks are a **per-vendor silo** locked to one agent. None offers a runtime-agnostic layer.

## Summary / Differentiator

Don't position on "hooks" — the field has converged on the mechanism. Position on **the only event-sourced, runtime-agnostic control plane that puts one audit (and later, one policy) layer over *every* agent CLI.** Because atelier sits above `RuntimeKind`, a single `[hooks.*]` config governs Codex *and* Claude *and* Cursor uniformly; because it is event-sourced, the audit trail is *inherent* — the history already **is** the log. Competitors structurally cannot follow a runtime-switching user. That is the moat; V1 is its smallest honest proof.

## Core Features

| # | Feature | Priority | Description |
|---|---|---|---|
| F1 | **Cross-runtime audit hook** | Critical | A `[hooks.<event>]` config dispatches a shell command on each event with a **normalized, versioned payload identical across all runtimes**. The flagship: one config, uniform audit over Codex/Claude/Cursor/zai. |
| F2 | **Decision-first payload contract** | Critical | A curated, frozen subset of lifecycle events with independently versioned payload schemas, shaped so the V2 veto adds a *return field*, not a redesign. Includes the `step_started → agent_step_started` rename before anyone depends on it. |
| F3 | **Off-funnel async dispatcher** | Critical | `record_event_with_group` `try_send`s onto a bounded channel drained by a dedicated task (reusing the existing `tokio` `Command` + `kill_on_drop` + timeout idiom). Never blocks the worker/render path; recursion blocked structurally via an `origin: hook` meta bit; backpressure drops/coalesces. |
| F4 | **Security posture** | High | Read hook commands **only from home/`ATELIER_CONFIG`** (ignore repo-local `./atelier.toml` → deletes RCE-on-clone). Payload is **stdin-only, no argv templating**. Data-minimization (metadata-only default, opt-in full payload). Hardened redaction; egress allowlist for built-in shipping. |
| F5 | **Minimal notification slot** | High | Unattended *push* on `approval_requested`, `clarification_requested`, and the terminal trio (`run_completed`/`run_failed`/`run_limit_reached`) — the user supplies the command (e.g. `notify-send`, `osascript`). |
| F6 | **`atelier events --follow`** | Medium | A stable JSON stream over the existing `.atelier/` JSONL — the cheap, pull-based DIY observability path, so the hook dispatcher only owns the turnkey/uniform/push jobs. |
| F7 | **Hook transparency in chat** | Medium | Hook executions are themselves recorded as `hook_started`/`hook_completed` events and projected into the transcript, with exit status — captured for audit, not silent. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| Cross-runtime audit adoption | **>20%** of active configs define ≥1 audit hook within 60 days | config scan / opt-in telemetry |
| Cross-runtime uniformity | **1 config governs all N configured runtimes** with zero per-runtime edits | integration test across Codex/Claude/Cursor/zai + payload-schema conformance |
| Hook execution reliability | **>99%** of dispatches complete without error/timeout | `hook_completed` event status field |
| Observer dispatch overhead | **<5ms** p99 added to the event write path (non-blocking) | instrument the `try_send` tap at `record_event_with_group` |
| Notify response time (secondary) | **−40%** median time-to-response on approval/clarification for notify users | Δ `approval_requested → approval_resolved`, segmented by hook-configured |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | **Strong** — fills the #1 governance gap and lays the V2 veto's rails; V1 is an increment, the veto is the transformative part |
| **Reach** | What % of users would this affect? | **Strong** — run-lifecycle events + completion notify appeal broadly; cross-runtime audit serves the governance segment (opt-in caveat) |
| **Frequency** | How often would users encounter this value? | **Strong** — fires on every run once configured |
| **Differentiation** | Does this set us apart or just match competitors? | **Strong** — uniform cross-runtime audit is the one thing single-vendor hooks structurally cannot copy |
| **Defensibility** | Easy to copy or compounds over time? | **Maybe** — the mechanism is copyable; the moat is the control-plane position + schema contract, which compounds but isn't ironclad |
| **Feasibility** | Can we actually build this? | **Strong** — observer dispatch reuses an existing idiom; the payload schema is the real design effort, and it's tractable |

**Leverage type:** Strategic Bet (V1 = first increment of a cross-runtime governance/integration platform).

## Council Insights

- **Recommended approach:** Observer-tier V1, re-anchored from notify-first to **uniform cross-runtime audit**, built on a **decision-first payload schema** that the V2 blocking veto reuses. Ship a minimal notify slot and `events --follow` alongside. Defer blocking to V2.
- **Key trade-offs:** Higher upfront design cost (the uniform schema is the hard part) in exchange for landing on the differentiated job and de-risking V2; notify demoted from flagship because it's parity and its idle-time metric is pinned near-zero by the `yolo` default.
- **Risks identified:** *Schema capture* (shaping the payload for the trivial notify case → wrong for the veto) → design decision-first against V2's needs. *Data egress* via home-config hooks → metadata-only stdin default + hardened redaction + egress allowlist. *Repo-local hook demand* (husky/pre-commit precedent) → record the trust-gate design now as the gating seam. *Hot-path stall* → off-funnel bounded-channel dispatcher, never await at the write site.
- **Stretch goal (V2+):** The audited **blocking cross-runtime governance gate** (deny-command / deny-write-outside-roots / require-approval), measured by *violations prevented*, on the same schema and dispatcher — the cell where portability is fully load-bearing and competitors can't follow.
- **Binding dissent (devils-advocate):** Observer V1 is only justified if its flagship demo is genuinely cross-runtime (**≥2 runtimes**), not a single-runtime notify toy. Treat that as an acceptance condition.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| Event write path (`record_event_with_group`, `src/app/mod.rs:4215`) | Tap via non-blocking `try_send` onto a bounded channel — not an inline spawn (runs per stream-delta token) |
| Async subprocess idiom (`src/app/git.rs:68`, `src/runtime/codex.rs`) | Reused by the dispatcher (`Command` + `kill_on_drop` + `select!` timeout) |
| `redact_sensitive_text` (`src/runtime/mod.rs:704`) | Hardened beyond `Bearer`/`sk-`/`zai-`; applied to payloads |
| Chat projection (`src/app/chat/projection.rs:58`) | New `hook_started`/`hook_completed` handlers (carry `origin: hook` so the tap ignores them — closes recursion + replay double-fire) |
| Config ladder (`src/config/mod.rs`) | Add `HooksConfig`/`RawHooksConfig`; drop the Local layer's hooks |
| `ApprovalMode` / `ActionDecision::Denied` (`src/actions/mod.rs:91`) | The V2 veto reuses these + the validated schema |

## Out of Scope (V1)

- **Blocking PreAction veto hooks** — deferred to V2; they belong on the action hot path and a security boundary, which is only safe *after* the cross-runtime schema is validated read-only.
- **Repo-local / project hooks + hash-pinned trust gate** — ignored in V1 to delete RCE-on-clone by construction; the trust-gate *design* is recorded for when project hooks land.
- **Sandboxing / seccomp of hook processes** — out; hooks run as user-owned shell (like `make`/`git` hooks). Data-minimization + egress allowlist substitute.
- **HTTP / LLM hook handlers** (à la Claude Code's endpoint/prompt handlers) — V1 is shell-command only.
- **Hook-driven parameter rewriting** (à la Cursor's `updated_input`) — out; observers cannot mutate the run.

## Architecture Decision Records

- [ADR-001: V1 ships cross-runtime observer hooks with a decision-first payload contract; blocking veto deferred to V2](adrs/adr-001.md) — re-anchors V1 from notify-first to cross-runtime audit, fixes the dispatcher off the event funnel, sets the V1 security posture, and sequences the blocking veto as V2 on the same schema.

## Open Questions

- **Exact frozen event set** for the public contract — which lifecycle events make the curated subset beyond the seven proposed (e.g. `session_started`, `workflow_completed`, `action_completed`)?
- **Default stdin granularity** — which metadata fields are in the metadata-only default, and what's the UX for opting into the full payload?
- **Built-in shipping vs pure dispatch** — does atelier ship an audit sink (implying the egress allowlist has teeth), or only dispatch and leave the `curl` to the user's hook command? The allowlist only binds atelier-owned shipping.
- **Telemetry** — do the adoption KPIs require telemetry atelier doesn't yet have? If so, what's the minimal opt-in mechanism?
- **Docs framing** — how do `events --follow` (pull, DIY) and the audit hook (push, turnkey) stay clearly distinct so they don't read as redundant?
