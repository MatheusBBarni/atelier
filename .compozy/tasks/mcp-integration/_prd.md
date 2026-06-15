# Product Requirements Document: Harness-Owned MCP Tool Access

## Overview

`atelier` routes one prompt across many agent-CLI runtimes (Codex, Claude, Cursor, Z.ai), but it cannot reach a single MCP (Model Context Protocol) server — it deliberately disables MCP so a model can't bypass the harness's approval boundary. The cost is that atelier's users are cut off from the ~10k-server MCP ecosystem (filesystem, git, GitHub, fetch, company tools) that the rest of the field depends on.

This feature gives the **solo power-user** one place to configure their MCP servers and have those tools work across **every** runtime, with each tool call brokered through atelier's existing approval, capability, and audit boundary — so the harness, not the model, decides what runs. It turns one integration into N runtimes' worth of capability, and turns atelier's trust boundary into a governance property no passive gateway or single-vendor host offers.

## Goals

- **Close the capability gap:** a configured MCP tool is usable from every enabled runtime, not one.
- **Make tool use trustworthy by default:** every call passes the harness boundary; read-only tools stay frictionless while write/command tools are gated until trusted.
- **Keep it consistent:** configuring and approving MCP tools should feel like the TOML/approval/doctor surfaces atelier users already know.
- **Prove value, locally:** measure trusted tool-call completions from the local event log, with no telemetry.
- **Milestones (phase gates, not dates):** MVP ships stdio + the full trust spine behind a flag; Phase 2 adds remote (HTTP/OAuth) + guided commands; Phase 3 adds team governance.

## User Stories

**Primary persona — "Portable Pat," the solo power-user**
- As a solo dev, I want to configure an MCP server once so that its tools work no matter which runtime or model a run uses.
- As a solo dev, I want read-only tools (search, read, list) to run without prompting so that I'm not buried in approvals.
- As a solo dev, I want a clear, one-time decision when a new server first wants to act, showing me what the tool actually does, so that I can trust it deliberately.
- As a solo dev, I want to promote a server I trust to run quietly — and revoke that later — so that I control friction without losing safety.
- As a solo dev, I want to choose which tools each agent can use so that an agent only has the access it needs and my context isn't flooded.
- As a solo dev, I want to see every MCP tool call and its result in the transcript so that I always know what ran.
- As a solo dev, I want a health check that tells me my servers are reachable and work across runtimes so that I can fix setup problems fast.

**Secondary persona — "Governed Gabi," the future team admin (context only; not a V1 target)**
- As a team admin, I want per-tool permissions and an audit export so that third-party servers are safe to sanction. *(Deferred to Phase 3; the V1 trust model is designed not to preclude it.)*

## Core Features

**Critical**
- **Cross-runtime tool access (CF1):** A configured MCP tool produces a successful, brokered call from every enabled runtime. The user configures once; the harness mediates the call uniformly.
- **TOML server configuration (CF2):** Users add, edit, and remove servers in a `[mcp.servers.<name>]` section (command/args/env; transport-agnostic, stdio in V1), with an `--init-config` example. Adding a server requires no code and no per-runtime setup.
- **Per-server trust + read-only auto-allow (CF3):** On first action from a server, the approval card shows the tool's **full, untruncated description and its origin server**; the user approves once and may promote the server to *trusted* (remembered, revocable). Read-only tools auto-allow; write/command tools prompt until the server is trusted; any later change to a tool's description re-prompts.
- **Per-agent tool allowlist, default-deny (CF4):** Each agent sees only the tools its config explicitly grants — least privilege and a bounded tool list in one control.

**High**
- **Auditable tool calls in the transcript (CF5):** Every MCP call and result renders as a chat item (call, argument summary, result, status), with secrets redacted before anything is written to local history.
- **MCP health + cross-runtime parity in `--doctor` (CF6):** Reports server reachability and config validity (distinguishing a real failure from harmless server log noise) and a runtimes × servers **parity matrix** that serves as the release signal.

**Medium**
- **Local usage readout (CF7):** Trusted-completion counts and parity status are derived locally and shown via `--doctor --json`; nothing leaves the machine.
- **Opt-in feature flag (CF8):** `features.mcp_enabled` turns the whole surface on; off by default.

## User Experience

**First run (configure → verify → first call → trust):**
1. Pat adds a few lines under `[mcp.servers.filesystem]` in `atelier.toml` (mirroring how they already add a runtime).
2. Pat runs `atelier --doctor`, which confirms the server starts and is reachable — and, if it doesn't, says *why* in plain language (e.g., binary path not found; the server is healthy but logging to stderr), avoiding the notorious false-"failed" trap.
3. On the first run that needs a filesystem tool, a **read** tool just works (auto-allowed). The first **write** tool surfaces the familiar yellow approval card — now showing the tool's full description and origin server — and Pat approves with `y`.
4. Pat promotes the `filesystem` server to *trusted*; subsequent calls from it run quietly. Pat can revoke that trust at any time.

**Day to day:** Pat switches runtimes freely; the same tools remain available with the same trust decisions. Read/search tools never prompt; write/command tools from untrusted servers do. Every call is visible in the transcript.

**Discoverability:** the `/help` Approvals tab explains MCP trust tiers; `--doctor` lists configured servers and their status; `--init-config` ships a commented `[mcp.servers.*]` example.

**Safety-forward details:** the approval card never truncates a tool description (the tool-poisoning defense), trust is always revocable (avoiding irreversible "always approve" traps), and a changed tool description forces re-approval.

## High-Level Technical Constraints

- **Local servers only in V1:** stdio (local subprocess) servers; remote/HTTP servers are not supported until Phase 2.
- **Data stays local:** no telemetry or egress; secrets are redacted before being written to local session history.
- **Harness-mediated by contract:** MCP tool calls are subject to the same approval and capability boundaries as every other action; the model cannot self-invoke tools.
- **Context budget:** adding servers must not flood the model's context — the per-agent allowlist bounds the exposed tool list; first-call latency should feel comparable to a command action.
- **Spec versioning:** pin to a specific MCP protocol revision and surface which one is in use.

## Non-Goals (Out of Scope)

- **Remote / HTTP servers and OAuth** — deferred to Phase 2 as one bundle (configuring a remote server's URL/auth has no effect in V1).
- **Guided `/mcp` commands** — V1 uses TOML; a command surface is Phase 2.
- **Team governance** — per-tool permission globs, audit export, curated/signed catalogs, and RBAC are Phase 3.
- **Server-initiated model calls (MCP sampling)** — out; it complicates the trust boundary.
- **Mid-run tool-catalog hot-reload** — out; the tool set is fixed per run for predictability.
- **Auto-install / registry fetch of servers** — out; V1 runs only user-configured servers (supply-chain safety).
- **Cross-user metric aggregation** — out; metrics are local-only.

## Phased Rollout Plan

### MVP (Phase 1)
- CF1–CF8 over stdio, behind `features.mcp_enabled`.
- **Success criteria to proceed:** parity matrix green across all runtimes on the reference server set; the small-model emission spike clears its long-tail bar; zero secret leaks into history; the trust/approval flow is usable end-to-end.

### Phase 2 (V1.1)
- Remote support: Streamable HTTP transport + OAuth (one bundle); the guided `/mcp add|list|status|trust` command surface.
- **Success criteria:** a remote server is reachable and governed identically to stdio; measured time-to-first-trusted-call drops.

### Phase 3 (V2+)
- Team governance: per-tool permission globs, one-command audit export, curated/signed server catalog, RBAC; deterministic tool-ledger / run replay.
- **Long-term success:** a team can adopt third-party servers under policy with an attributable audit trail.

## Success Metrics

| Metric | Target | Lens |
|---|---|---|
| **Trusted tool-call completions / active user** *(North Star)* | Median ≥ 10 / week | Value delivered (local event log) |
| Cross-runtime parity *(release gate)* | ≥ 95% of reference servers succeed on **every** runtime | Capability parity |
| Boundary containment | 0 calls bypass approval/capability; 100% audited | Trust integrity |
| Redaction safety | 0 secret leaks into local history | Privacy |
| Small-model emission reliability | ≥ 95% p95 (incl. one repair) on Z.ai's smallest model | Parity feasibility |
| Time-to-first-trusted-call | < 5 min median | Onboarding friction |
| Approval friction | Read-only tools = 0 prompts; write/command prompts trend down per trusted server | Fatigue avoidance |

## Risks and Mitigations

- **Config friction / silent setup errors** (a top MCP onboarding failure) → ship an `--init-config` example and actionable `--doctor` diagnostics that name the real cause and distinguish failure from log noise.
- **Approval fatigue** (93% of prompts get rubber-stamped industry-wide) → read-only auto-allow + per-server *remember* + revocable trust, instead of prompting every call.
- **Competitive commoditization** ("supports MCP" is everywhere; gateways already do "configure once") → lead on the intersection only atelier holds: multi-runtime **and** harness-owned invocation; ship the trust-legibility differentiators.
- **Erosion of trust via tool poisoning** → surface full tool descriptions at approval, re-prompt on change, and contain blast radius with default-deny.
- **MCP spec churn** (transports/auth changed three times in 2025) → pin and surface a revision; keep config transport-agnostic so remote is additive.
- **Parity broken on weak models** (small models may mis-emit tool calls) → gate the build on the emission spike; degrade-not-abandon per runtime rather than failing the whole run.

## Architecture Decision Records

- [ADR-001: Broker MCP through the harness ActionRequest contract](adrs/adr-001.md) — build a harness-owned client (not native per-CLI MCP or an external gateway).
- [ADR-002: stdio-first V1; defer HTTP + OAuth as one bundle to V1.1](adrs/adr-002.md) — ship the broker/governance spine over stdio; keep config transport-agnostic so V1.1 is additive.
- [ADR-003: Config-first MVP product surface for V1](adrs/adr-003.md) — TOML config + existing approval card + per-agent allowlist + doctor parity matrix; defer `/mcp` commands and team governance.

## Open Questions

- **Emission-spike pass bar:** which reference server (tool count), what p95 threshold, and confirmation the repair round counts (the council's standing dissent).
- **Reference-server set** to officially document and gate parity against (e.g., filesystem, git, github, fetch?).
- **Read-only classification source:** which signal marks a tool "read-only" for auto-allow (resource-vs-tool split, tool annotations) — needs confirmation.
- **Trust-state storage:** where revocable per-server trust lives (config vs. session state) — a techspec decision.
- **V1.1 trigger** and which MCP spec revision to pin.
- **First-contact approval card layout:** how much of a long description to show inline vs. expand.
