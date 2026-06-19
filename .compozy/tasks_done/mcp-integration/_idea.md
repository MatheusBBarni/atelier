# MCP Tools as a Harness-Owned, Capability-Gated Action Surface

## Overview

- **Problem:** `atelier` deliberately strips MCP from its runtimes (`--strict-mcp-config`, `--tools ""` in the Claude runtime) so models can't self-invoke tools — leaving it with **zero** access to the ~10k-server MCP ecosystem that Claude Code / Cursor users depend on.
- **Who:** The primary user is the **solo power-user** ("Portable Pat") who runs atelier locally, wires up their own MCP servers (filesystem, git, GitHub, fetch, a company tool), and wants that toolset available no matter which runtime or model a run routes to.
- **Why valuable:** atelier brokers MCP calls through its **own** action contract, so one config lights up MCP on **every** runtime (Codex, Claude, Cursor, Z.ai) at once, and every third-party tool inherits approval gating, capability allowlists, and event audit for free — a property no passive gateway or single-agent host can structurally match.
- **V1 ambition:** A **Strategic Bet**, scoped tight: ship the full broker + governance spine over **stdio transport only**, with HTTP + OAuth committed as one additive V1.1 bundle.

## Summary / Differentiator

"Supports MCP" is commoditized — every coding agent has it. The unoccupied white space is an **intersection**: (a) one config spanning multiple *agent runtimes* **and** (b) a harness that *owns the tool invocation* through its approval / capability / audit boundary. Gateways (McpMux, IBM ContextForge, 17+ others) deliver "configure once" but only as passive proxies the *model* still self-invokes through; single-agent hosts (Goose) are model-agnostic but are one agent, not a layer over many CLIs. atelier's lead sentence carries both moats: **configure your MCP tools once and every runtime can use them — while the harness, not the model, decides which calls actually run.**

## Problem

atelier's core safety design is that agents never touch the filesystem or tools directly — they emit `ActionRequest`s gated by capabilities, workspace roots, and approval mode. To preserve that boundary, the Claude runtime *intentionally* disables MCP so a stray `tool_use` can't bypass the harness. The side effect is total: atelier today cannot reach a single MCP server, while the rest of the ecosystem has standardized on MCP as the way agents reach filesystems, GitHub, Postgres, web fetch, and company-internal tools.

For the solo power-user this is a daily tax. They already maintain MCP servers for their other tools; every CLI needs its own MCP config, and atelier reaches none of them. The workaround is to drop out of atelier to whichever single-vendor CLI happens to support the tool they need — which defeats atelier's whole reason to exist (one harness, many runtimes). Worse, the ecosystem's own governance story is thin: integrated hosts have shallow per-host approval and no real audit trail, and full governance exists only as separately-deployed gateways the model still self-invokes through.

The opportunity is that atelier *already has* the missing trust machinery. An MCP tool call can become just another `ActionRequest` — the model returns the same JSON contract it already uses, and the harness brokers the real call through `validate → approval → execute → events → projection`. That turns one MCP integration into N runtimes' worth of capability, and turns atelier's existing approval/capability/audit boundary into a governance story no raw MCP host offers.

### Market Data

- MCP is now **vendor-neutral and large**: ~9,650 servers in the official registry (~18–21k across third-party directories), 97M+ monthly SDK downloads, all four majors (Anthropic/OpenAI/Google/Microsoft) on board; donated to the Linux Foundation's Agentic AI Foundation in Dec 2025. ~41% of surveyed enterprises run MCP in production (Stacklok 2026, n=100).
- **"Supports MCP" is fully commoditized** across Cursor, Cline, Windsurf, Zed, Continue, Copilot, and Goose — it is not a differentiator on its own.
- **Closest competitors:** Goose (model-agnostic, but a single agent) and gateways McpMux / IBM ContextForge (configure-once, but passive proxies; 17+ already exist).
- **Transports:** stdio = table stakes; Streamable HTTP = recommended for remote; **SSE deprecated (2025-03-26)**; OAuth 2.1 "server-as-Resource-Server" model (2025-06-18).
- **Security is the headline risk:** tool poisoning (CVE-2025-54136), MCP Inspector RCE (CVSS 9.4), the Supabase/Cursor token leak, an NSA advisory; plus approval fatigue and tool-list context bloat.

## Core Features

| #  | Feature | Priority | Description |
|----|---------|----------|-------------|
| F1 | Harness-brokered MCP action contract | Critical | New `CallMcpTool` (effect) and `ReadMcpResource` / `ListMcpResources` (read) `ActionRequest`s. The model emits the same `action_request` JSON; the harness brokers the real MCP call through `validate → ActionDecision → approval → execute → events → projection`. |
| F2 | `[mcp.servers.*]` config + connection supervisor | Critical | Transport-agnostic `[mcp.servers.<name>]` config (stdio wired in V1) flowing through the Raw→Merged→Effective ladder. A dedicated `McpSupervisor` owns async, event-recorded connection lifecycle (spawn / health / timeout / kill) — not `App`. |
| F3 | Capability gating + default-deny allowlist | Critical | New `Capability::McpTool` plus a string-keyed, per-`(agent, server, tool)` **default-deny** allowlist. Dynamic tool names never widen the static type system (closed enum stays closed). |
| F4 | Record-time payload redaction + content caps | Critical | Secrets redacted from **full-size** payloads *before* durable persistence to `.atelier/` (not display-only), reusing `redact_sensitive_text`; the 8KB `CONTENT_PREVIEW_CAP_BYTES` governs chat display. |
| F5 | Event-recorded tool-catalog snapshot in prompt | High | `build_orchestrator_prompt` advertises tools from an immutable, event-recorded **snapshot**, never live connection state — preserving replay determinism. Enforced by an ADR-level invariant + guard test (à la `colors_live_only_in_theme_module`). |
| F6 | Per-server trust tiers + first-contact approval | High | No silent `yolo` on first contact: a new server is untrusted → informed approval that **surfaces the real tool description** → explicit promote to trusted. Pin a hash of `(name + description + input schema)`; diff loudly on change → re-approval. |
| F7 | Emission-reliability spike + schema repair loop | High | Pre-build spike on Z.ai's smallest model against a high-tool-count server (p95 emission *with* a repair round). Harness-side schema validation + structured repair re-prompt; per-runtime "degrade-not-abandon" flag; per-agent tool-list scoping to bound prompt bloat. |
| F8 | Feature flag + cross-runtime parity doctor | Medium | `features.mcp_enabled` gate (mirrors `parallel_step_groups`). `atelier --doctor` runs a parity matrix (runtimes × reference servers) that serves as the release gate. |

## KPIs

| KPI | Target | How to Measure |
|-----|--------|----------------|
| **Trusted tool-call completions / active user** *(North Star)* | Median ≥ 10 gated-and-completed MCP calls per active user per week | Local derivation over `.atelier/` event log; self-visible `--doctor --json` readout (no telemetry) |
| Cross-runtime parity *(release gate)* | ≥ 95% of the reference server set returns a successful brokered call from **every** enabled runtime; red blocks ship | `--doctor` parity matrix (runtimes × servers) |
| Boundary containment | **0** MCP executions bypass `validate→capability→approval`; 100% have paired `mcp_tool_called`+`result` events | Event-store invariant assertion |
| Redaction safety | **0** secret leaks into durable `.atelier/` payloads | Known-secret injection test + sampled audit |
| Small-model emission reliability | ≥ 95% p95 well-formed `CallMcpTool` (incl. one repair round) on Z.ai's smallest model vs. the gnarly reference server | Emission spike harness |
| Time-to-first-trusted-call | < 5 min median from adding `[mcp.servers.x]` to first completed gated call | Doctor / onboarding timing |

## Feature Assessment

| Criteria | Question | Score |
|----------|----------|-------|
| **Impact** | How much more valuable does this make the product? | **Strong** — unlocks ~10k-server ecosystem for every runtime at once |
| **Reach** | What % of users would this affect? | **Strong** — most power users want MCP; opt-in flag tempers it |
| **Frequency** | How often would users encounter the value? | **Strong** — wired servers (fs/github) hit on most runs |
| **Differentiation** | Set us apart or just match? | **Strong** — on the intersection; "supports MCP" alone is **Pass** |
| **Defensibility** | Easy to copy or compounds? | **Maybe** — architectural moat is real but standards-based; event-sourcing + governance compound |
| **Feasibility** | Can we actually build it? | **Strong** — clean seams confirmed; lifecycle + small-model emission are the hard parts |

Leverage type: **Strategic Bet** with compounding governance/event-sourcing upside.

## Council Insights

- **Recommended approach:** Build the harness-owned broker — the only single mechanism spanning all five runtimes (native function-calling would *fork* the contract; the Z.ai and `fake` runtimes have no `tools` array, and atelier already A/B-tested native tool exposure via the Claude strip and found it *degraded* small-model adherence). Ship stdio-first; defer HTTP+OAuth as one additive V1.1 bundle. Lead with both moats; North Star = locally-derived trusted-completion; parity = release gate.
- **Key trade-offs:** harness-brokered JSON emission vs. native constrained decoding; "Complete" V1 vs. stdio-first; multi-runtime-lead vs. trust-boundary-lead (resolved: both, in one sentence).
- **Risks identified:** (1) small models botching `CallMcpTool` JSON could break parity → emission spike bound to the long tail + repair loop + degrade-not-abandon flag; (2) un-stripping a deliberately-built boundary → record-time redaction, default-deny per-`(agent,server,tool)`, trust tiers, token-audience handling; (3) long-lived stateful connections vs. on-demand runtimes → dedicated `McpSupervisor`; (4) live tool list vs. replay determinism → event-recorded catalog snapshot.
- **Stretch goal (V2+):** **Governed MCP catalog for teams** — curated/signed catalog, per-server trust tiers, RBAC, one-command audit export (the "Gabi" persona). The V1 governance taxonomy ships specifically to keep this reachable without rework. Compounding follow-on flagged: a **deterministic MCP tool ledger + run replay** that exploits atelier's event-sourcing moat.

## Integration with Existing Features

| Integration Point | How |
|-------------------|-----|
| `ActionRequest` contract / `execute_action_request` | New `CallMcpTool` / `ReadMcpResource` / `ListMcpResources` arms |
| `Capability` + `ToolName` allowlist | New `Capability::McpTool` + string-keyed `(server, tool)` default-deny allowlist |
| Config ladder (Raw→Merged→Effective, `PrintableConfig`) | New `[mcp.servers.*]` section + credential redaction |
| `build_orchestrator_prompt` | Event-recorded tool-catalog snapshot section |
| Event sourcing + chat projection | New `mcp_tool_called` / `mcp_tool_result` kinds + `apply_*` arms; record-time redaction |
| Runtime ownership in `App` | Parallel `McpSupervisor` owning connection lifecycle |
| Claude runtime MCP strip | Replaced by harness-brokered calls; regression test asserts the model still can't self-invoke |
| Feature flags (`features.parallel_step_groups`) | New `features.mcp_enabled` mirroring it |

## Out of Scope (V1)

- **Streamable HTTP / remote transport** — deferred to V1.1 as one coupled bundle with OAuth (ADR-002); unvalidatable without a real remote server, and HTTP-without-auth is a useless half-thing.
- **OAuth 2.1 / remote authorization** — ships with HTTP in V1.1; token-audience handling is designed-in but not built.
- **Governed team catalog / RBAC / signed catalog** — V2+ stretch; V1 targets the solo user (taxonomy ships to keep it reachable).
- **MCP sampling / server-initiated LLM calls** — out; a reverse channel complicates the trust boundary.
- **Mid-run hot-reload of the tool catalog** — out; snapshot-per-run preserves replay determinism (`tools/list_changed` reflected at the next snapshot, not mid-run).
- **Auto-install / registry fetch of arbitrary servers** — out; supply-chain risk. V1 is user-configured servers only.

## Architecture Decision Records

- [ADR-001: Broker MCP through the harness ActionRequest contract](adrs/adr-001.md) — build a harness-owned client (not native per-CLI MCP or an external gateway), because it's the only mechanism uniform across all five runtimes.
- [ADR-002: stdio-first V1; defer HTTP + OAuth as one bundle to V1.1](adrs/adr-002.md) — ship the broker/governance spine over stdio only; make config transport-agnostic, `connect` async/event-extensible, and the governance taxonomy V1 work so V1.1 is additive.

## Open Questions

- **Emission-spike pass bar** (devils-advocate's standing dissent): which reference server (tool count), what exact p95 threshold, and confirmation the repair round is included — a friendly spike manufactures false confidence.
- **V1.1 trigger** for the HTTP/OAuth bundle, and which MCP spec revision to pin (transports + auth were revised three times in 2025).
- **North Star aggregation:** does trusted-completion ever need opt-in cross-user aggregation, or stay purely local/self-visible?
- **Tool-list scoping policy:** default per-agent allowlist, and how a solo user discovers/enables tools without prompt bloat.
- **First-contact approval UX:** surfacing tool description + schema diff on promotion without triggering approval fatigue.
