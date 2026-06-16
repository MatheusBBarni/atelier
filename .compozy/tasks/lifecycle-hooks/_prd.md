# PRD — Lifecycle Hooks (V1: Observer Tier)

## Overview

atelier records every step of every run as a durable event, but nothing outside the process can react to those events. Lifecycle Hooks adds a supported extension point: users map lifecycle events (run started, approval needed, file edited, run completed/failed, …) to actions that fire when those events occur.

V1 serves the **solo developer** first: a built-in, cross-platform notification that pings them when a run needs them or finishes — working even over SSH — plus the ability to run their own shell command on any event (e.g. format-on-edit) and a live `atelier --events follow` stream. Every hook receives a **stable, versioned payload that is identical across all runtimes** (Codex, Claude, Cursor, zai), so behavior is consistent no matter which runtime a project uses — and so a team can later wire one integration instead of four. The blocking/policy tier is deliberately deferred to V2 and reuses the same payload.

## Goals

- Give users a supported way to **react to what atelier does** (notify, automate, observe) without scraping log files.
- Make "**notify me when my run needs me or finishes**" work out-of-the-box, including over SSH.
- Establish a **stable, uniform cross-runtime event payload** that hooks — and the future policy tier — build on.
- Keep it **private by default**: secrets redacted, metadata-only payloads by default, nothing leaves the machine unless the user's own command sends it.
- Measure adoption **locally** (via `--doctor`), honoring the audience's opt-in/privacy norms.

## User Stories

**Primary — Solo developer**
- As a solo dev, I want a desktop notification when my run pauses for approval or finishes, so I can step away and not babysit the terminal.
- As a solo dev, I want notifications to work over SSH with no extra setup, so remote sessions still reach me.
- As a solo dev, I want to run a quick script on each file edit (e.g. format/lint), so my edits stay consistent without manual steps.
- As a solo dev, I want to watch a live JSON stream of what atelier is doing, so I can debug or prototype an integration fast.

**Secondary — Platform / DevEx engineer**
- As a DevEx eng, I want one hook config that emits the same normalized payload regardless of runtime, so I can wire atelier into our observability/audit with a single integration.
- As a DevEx eng, I want copy-paste recipes (append-to-file, webhook), so I can stand up an audit trail in minutes.

**Secondary — Eng manager / security lead**
- As an eng manager, I want a consistent record of agent actions across runtimes, so I can answer "what did the agent do?" for review.
- *(V2-facing)* As a security lead, I want to enforce policy so out-of-policy actions are blocked — explicitly deferred to V2.

## Core Features

| # | Feature | Priority | What it does |
|---|---|---|---|
| F1 | **Lifecycle hook dispatch** | Critical | Map any supported event to a shell command (per-event config); the command receives the event payload as JSON on stdin. |
| F2 | **Built-in cross-platform notifier** | Critical | Declarative notification on chosen events (`notify = true`), working on macOS/Linux and **over SSH**, with sane noise-suppression defaults — no per-OS scripting required. |
| F3 | **Uniform cross-runtime payload** | Critical | A stable, versioned, documented JSON shape **identical across all runtimes** — the contract every hook (and the V2 policy tier) relies on. |
| F4 | **Live event stream (`atelier --events follow`)** | High | A JSON stream of lifecycle events that doubles as the dry-run/test harness for authoring hooks. |
| F5 | **Recipes & scaffolding** | High | Bundled copy-paste recipes (notify, append-to-audit-file, webhook); `--init-config` includes commented hook examples; `/config` and `--doctor` surface active hooks. |
| F6 | **Privacy-safe payloads** | High | Metadata-only on stdin by default, opt-in to full payload; secrets redacted before a hook ever sees them. |
| F7 | **Transparency in transcript** | Medium | Hook executions appear in the chat transcript with their status — captured, not silent. |

## User Experience

**Discover →** A user learns hooks exist from `--init-config` (which scaffolds commented examples), the README config section, and `/config`. **Configure →** They add a single `[hooks.<event>]` block to their own config — one file, one format, consistent with `[agents]`/`[council]`. **Test →** They run `atelier --events follow` to watch real payloads and confirm the hook fires before relying on it. **Use →** Notifications "just work" (including SSH); their own commands run on the events they chose. **Observe →** `atelier --doctor` shows which hooks are configured and when each last fired.

Onboarding target: a brand-new user gets a working notify hook in **under five minutes** from the scaffolded example. Cross-platform: macOS and Linux desktop, plus SSH sessions; the tmux-strips-notifications case is handled (passthrough or graceful fallback).

## High-Level Technical Constraints

- Hooks must **never perceptibly slow or block a run** — observer dispatch is fire-and-forget.
- Behavior must be **uniform across all runtimes**.
- **Privacy:** payloads redact secrets and are metadata-only by default; no data leaves the machine unless the user's own command sends it.
- **Security:** hook commands are read **only from the user's own (home) config**, never from repo-local project files — cloning a repo cannot run code; payloads are delivered on stdin only (never interpolated into the command).
- **Notifications must work over SSH** and degrade gracefully where unsupported.
- **No new telemetry/phone-home**; adoption is observable locally.

## Non-Goals (Out of Scope for V1)

- **Blocking / veto (policy-as-code) hooks** — V2, on the same payload.
- **Built-in shipping/exporter** to remote endpoints (webhook/OTel) + egress allowlist — V2; recipes cover this meanwhile.
- **Repo-local / project hooks + trust model** — deferred; V1 reads hooks only from user config.
- **Sandboxing/isolation** of hook processes — out; hooks run as user-owned shell, like `git`/`make` hooks.
- **HTTP / LLM hook handler types** — V1 is shell-command + the built-in notifier only.
- **Hook-driven mutation of the run** (parameter rewriting) — observers cannot change behavior.

## Phased Rollout Plan

### MVP (Phase 1)
Hook dispatch (F1), built-in notifier (F2), uniform payload (F3), `--events follow` (F4), recipes/scaffolding (F5), privacy-safe payloads (F6), transcript transparency (F7), and the security posture above.
**Proceed criteria:** the same hook config behaves identically across **≥2 runtimes**; notify verified on macOS + Linux + SSH; no perceptible run overhead; ≥99% hook reliability; new-user activation under 5 minutes.

### Phase 2
**Blocking PreAction veto** (policy-as-code) on the same payload and dispatcher, reusing the approval plumbing.
**Proceed criteria:** out-of-policy actions blocked, with zero false-block regressions on normal runs.

### Phase 3
**Integration hub** (built-in shipping/OTel exporter + egress allowlist); **repo-local/project hooks + trust model**; opt-in telemetry if warranted.

## Success Metrics

| Metric | Target | Measured (locally) |
|---|---|---|
| Notify coverage | Works on macOS + Linux + SSH (incl. tmux passthrough) | support matrix / manual verification |
| Run overhead | No perceptible slowdown (<5ms p99 added, non-blocking) | local benchmark |
| Hook reliability | >99% of dispatches complete without error/timeout | `--doctor` / event log |
| Cross-runtime uniformity | Identical payload shape across ≥2 runtimes | conformance check |
| Activation | Working notify hook in <5 min from `--init-config` | onboarding walkthrough |
| Privacy | 0 known secret-leak reports; metadata-only default verified | review + redaction tests |
| Adoption | `--doctor` reports hooks configured + last-fired | local signal + community feedback (no phone-home) |

## Risks and Mitigations

- **"Me-too" perception** (competitors already ship hooks) → lead with the notify polish (SSH "just works") + cross-runtime uniformity + bundled recipes; position as the cross-runtime control plane, not "we have hooks."
- **Low configuration** (opt-in power feature) → scaffold examples in `--init-config`, ship recipes, surface in `--doctor`, target <5-min activation.
- **Privacy/trust** (shell on agent events; payload egress via user commands) → metadata-only default, redaction, ignore repo-local hooks, document the leak path.
- **Notification fatigue** → noise-suppressing defaults; per-event opt-in.
- **Cross-platform/SSH gaps** (tmux strips notifications) → passthrough + OS fallback; SSH+tmux treated as first-class.
- **Scope creep toward the "platform" framing** → V1 stays observer + one battery; ADRs gate the rest to V2/V3.

## Architecture Decision Records

- [ADR-001: V1 ships cross-runtime observer hooks with a decision-first payload contract; blocking veto deferred to V2](adrs/adr-001.md) — sets the V1 scope, the dispatcher seam, the security posture, and the V2 sequencing.
- [ADR-002: V1 ships a thin hook dispatcher plus one built-in battery (cross-platform notifier); audit/webhook stay recipe-based; built-in shipping deferred](adrs/adr-002.md) — chooses the "batteries" level for V1.

## Open Questions

- Exact **frozen set of public events** (beyond the initial seven) and the fields each payload carries.
- The **default metadata-only field set** and the UX for opting into the full payload.
- **Notifier noise defaults** — which events notify by default; offer an "only when terminal unfocused" option?
- **Recipe boundary** — which recipes ship in `--init-config` vs the README.
- **CLI spelling** for the event stream given atelier's flag-based CLI (`--events follow` vs `--events-follow`).
- Whether the notifier's **tmux passthrough** is auto-detected or documented as a one-time setup step.
