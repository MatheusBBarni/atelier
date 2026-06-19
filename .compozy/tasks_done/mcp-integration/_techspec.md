# Technical Specification: Harness-Owned MCP Tool Access

## Executive Summary

This spec implements the PRD by adding a harness-owned MCP client whose tool calls reuse atelier's existing `ActionRequest` contract. Three new action kinds (`CallMcpTool`, `ReadMcpResource`, `ListMcpResources`) flow through the unchanged `validate → ActionDecision → approval → execute → events → projection` pipeline. A dedicated **`McpSupervisor`** actor owns long-lived stdio connections (via the official **`rmcp`** SDK, wrapped behind an internal `McpClient` trait); callers reach it through a cloneable `McpHandle` carried on `ActionExecutionContext`. A new `[mcp.servers.*]` config section mirrors `[runtimes.*]`, gated by a `features.mcp_enabled` flag. Per-server trust and tool-description pins live in an app-managed `.atelier/mcp-trust.json`; secrets are redacted **at record time** (new work — today's redaction is display-only).

**Primary trade-off:** routing every MCP call through the harness's JSON action contract (rather than each runtime's native function-calling) is what delivers uniform cross-runtime behavior and the trust boundary — at the cost of depending on the model to emit well-formed `CallMcpTool` JSON, which is gated by a pre-build emission spike and a structured repair loop rather than assumed.

## System Architecture

### Component Overview

- **`src/mcp/` (new module)** — owns the MCP client. `McpClient` trait (test seam over `rmcp`), `McpSupervisor` actor + `McpHandle`, `ToolCatalog` snapshot, `McpTrustStore`. Boundary: everything network/subprocess-facing lives here; nothing leaks into `App`.
- **`src/actions/mod.rs` (modified)** — three new `ActionKind` arms; validation maps them to capabilities + trust gate; execution dispatches through `McpHandle`.
- **`src/config/mod.rs` (modified)** — `[mcp.servers.*]` Raw→Merged→Effective structs; `features.mcp_enabled`; printable redaction.
- **`src/orchestrator/mod.rs` (modified)** — advertises tools from a recorded catalog **snapshot**, never live state.
- **`src/app/` (modified)** — owns the supervisor lifecycle; extends the approval flow with description-surfacing + trust promote/revoke; projects `mcp_*` events.
- **`src/doctor/mod.rs` (modified)** — per-server health checks + a runtimes×servers parity matrix + local metrics.
- **`src/runtime/claude.rs` (modified)** — keeps the MCP strip (the model still can't self-invoke); a regression test asserts it.

**PRD → component mapping:** CF1 cross-runtime → action contract + supervisor; CF2 config → `src/config`; CF3 trust/read-only → validation + `McpTrustStore` + approval card; CF4 allowlist → `has_tool`/`Capability::McpTool`; CF5 auditable calls → events + projection + record-time redaction; CF6 doctor/parity → `src/doctor`; CF7 local metrics → `--doctor --json`; CF8 flag → `features.mcp_enabled`.

**Data flow (a tool call):** model emits `CallMcpTool` JSON → `validate_action_request_with_scope` (capability + allowlist + trust tier) → `ActionDecision` → (approval card if untrusted, showing the full description) → `execute_action_request` sends `CallTool` over `McpHandle` → supervisor dispatches to the `rmcp` connection → result redacted at record time → events → chat projection.

## Implementation Design

### Core Interfaces

The new action kinds extend the existing enum (params stay untyped `serde_json::Value`):

```rust
// src/actions/mod.rs
pub enum ActionKind {
    // ...existing 7 (ReadFile … RecordNote)...
    CallMcpTool,     // params: { "server", "tool", "args" } — Capability::McpTool
    ReadMcpResource, // params: { "server", "uri" }          — read capability
    ListMcpResources // params: { "server" }                 — read capability
}
```

The supervisor is reached only through a cloneable handle; the actor owns connections:

```rust
// src/mcp/supervisor.rs
#[derive(Clone)]
pub struct McpHandle { tx: mpsc::Sender<McpCommand> }

pub(crate) enum McpCommand {
    CallTool   { server: String, tool: String, args: Value, reply: oneshot::Sender<Result<McpToolResult>> },
    ReadResource { server: String, uri: String, reply: oneshot::Sender<Result<McpResource>> },
    SnapshotCatalog { reply: oneshot::Sender<ToolCatalog> },
    Shutdown   { reply: oneshot::Sender<()> },
}
```

`rmcp` is wrapped behind a trait so the supervisor and tests never bind to the SDK surface:

```rust
// src/mcp/client.rs  (real impl adapts rmcp; fake impl is the in-repo stdio server)
#[async_trait]
pub trait McpClient: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<McpTool>>;
    async fn call_tool(&self, tool: &str, args: Value) -> Result<McpToolResult>;
    async fn read_resource(&self, uri: &str) -> Result<McpResource>;
}
```

Trust + pins are an app-managed, workspace-scoped store consulted by validation:

```rust
// src/mcp/trust.rs  (persisted to .atelier/mcp-trust.json)
pub enum TrustTier { Untrusted, Trusted }
pub struct McpTrustStore { /* in-memory cache of the file */ }
impl McpTrustStore {
    pub fn tier(&self, server: &str) -> TrustTier;
    pub fn pin(&self, server: &str, tool: &str) -> Option<ToolHash>; // (name+desc+schema)
    pub fn promote(&mut self, server: &str) -> Result<()>; // writes file + emits event
    pub fn revoke(&mut self, server: &str) -> Result<()>;
}
```

### Data Models

- **`McpServerConfig` (effective):** `{ id, transport: Stdio|Http, command, args, env: Map, url? }`. V1 wires `Stdio` only; `url`/`Http` parse but are inert (ADR-002).
- **`ToolCatalog`:** an immutable snapshot `{ servers: [{ server, tools: [{ name, description, input_schema, annotations }] }] }`, recorded as an event and read by the prompt builder.
- **`McpToolResult`:** `{ content: Value, is_error: bool }`, redacted before becoming an event; mapped into the existing `ActionResult { status, summary, content, diagnostic }`.
- **Trust store JSON:** `{ servers: { "<id>": { tier, pins: { "<tool>": "<hash>" } } } }`.
- **New event kinds (free-form `kind` string):** `mcp_tool_called`, `mcp_tool_result`, `mcp_catalog_snapshot`, `mcp_server_trusted`, `mcp_server_revoked`.

### Action & Config Surface

(No HTTP API; the "surface" is the action contract + config + doctor.)
- **Actions:** `CallMcpTool {server, tool, args}`, `ReadMcpResource {server, uri}`, `ListMcpResources {server}`.
- **Config:** `[mcp.servers.<name>]` with `transport`/`command`/`args`/`env`; `[features] mcp_enabled = true`; `--init-config` ships a commented example.
- **Doctor:** checks `mcp_server.<id>` (reachability/handshake, distinguishing failure from stderr noise) and `mcp_parity` (matrix + local trusted-completion count in `context`).

## Integration Points

- **`rmcp` SDK + MCP servers:** stdio subprocess in V1 via `TokioChildProcess`; `initialize` handshake performed by `serve`. **Auth:** none in V1 (local process); `env` values resolve `${VAR}` references, never inlined into history. **Errors/retry:** supervisor applies health/timeout/kill (the `cursor.rs` `tokio::select!` + `start_kill`→`kill` pattern); a hung server fails its calls without hanging the run; per-runtime **degrade-not-abandon** keeps a run alive if one runtime can't emit a call.

## Impact Analysis

| Component | Impact | Description and Risk | Required Action |
|-----------|--------|----------------------|-----------------|
| `src/mcp/*` | new | New module: client/supervisor/trust/catalog. Med risk (new subsystem) | Build behind `McpClient` trait + fake server |
| `src/actions/mod.rs` | modified | 3 new `ActionKind` arms in enum, `tool_name_for_action`, validate, execute. Low risk (additive arms) | Add arms; exhaustive matches updated |
| `src/config/mod.rs` | modified | `[mcp.servers.*]` ladder + flag + redaction. Low risk | Mirror runtime structs |
| `src/app/mod.rs` | modified | Owns supervisor lifecycle; approval card + trust promote/revoke; record-time redaction. **Med-high risk** (touches approval + durable write) | Careful tests; redaction gate |
| `src/orchestrator/mod.rs` | modified | Catalog-snapshot advertisement. Med risk (determinism) | Guard test: prompt reads snapshot, not handle |
| `src/app/chat/projection.rs` | modified | `apply_*` arms for `mcp_*` events; 8KB display cap reused. Low risk | Add arms |
| `src/doctor/mod.rs` | modified | Per-server checks + parity matrix. Low risk | Add checks |
| `src/runtime/claude.rs` | modified | Unchanged behavior; add regression test for the strip | Assert model still can't self-invoke |
| `Cargo.toml` | modified | New `rmcp` dependency (pinned). Med risk (pre-1.0 churn) | Pin; isolate behind trait |

## Testing Approach

### Unit Tests
- Validation arms: capability mapping, default-deny allowlist, trust-tier gate, description-hash diff → `RequiresApproval`.
- `McpTrustStore` promote/revoke/persist/reload; record-time redaction (inject a known secret, assert it never reaches the on-disk payload).
- Supervisor command handling against a **mocked `McpClient`**.
- Boundaries: mock at the `McpClient` trait; no subprocess.

### Integration Tests
- **In-repo fake stdio MCP server** (read-only resource, read-only-annotated tool, effect tool, description-mutating tool) spawned over real stdio — exercises the `initialize` handshake, a full `CallMcpTool` round-trip, resource auto-allow, and F6 diff-on-change re-prompt via `FakeRuntime`.
- Orchestrator snapshot-determinism guard test.
- Claude-strip regression (model cannot self-invoke).
- **`#[ignore]`d live tier** behind `MULTIAGENT_RUN_MCP_INTEGRATION=1` (real `server-everything`).
- **Emission spike** (separate gated harness): smallest Z.ai model × high-tool-count server, p95 emission *with repair*.

## Development Sequencing

### Build Order
1. **Add `rmcp` + `McpClient` trait + in-repo fake stdio server** — no dependencies.
2. **`[mcp.servers.*]` config + `features.mcp_enabled` + printable redaction** — no dependencies.
3. **`McpSupervisor` actor + `McpHandle`**, consuming config — depends on 1, 2.
4. **New `ActionKind`s + validation arms** (`Capability::McpTool`, read mapping, default-deny allowlist) **+ `McpTrustStore`** — depends on 1.
5. **`execute_*` arms via `McpHandle` + record-time redaction at event write** — depends on 3, 4.
6. **Catalog snapshot event + orchestrator advertisement + determinism guard test** — depends on 3.
7. **Approval-card extension (description surfacing, trust promote/revoke) + `mcp_*` projection arms** — depends on 4, 5.
8. **Doctor MCP checks + parity matrix + local metrics** — depends on 3, 4.
9. **Emission spike harness + claude-strip regression + redaction test** — depends on 5, 6, 7.

### Technical Dependencies
- `rmcp` crate (pinned version) available on crates.io.
- Pin a specific MCP spec revision and surface it in `--doctor`.
- The in-repo fake server must exist before step 5 can be E2E-tested.

## Monitoring and Observability

- **Events (durable, redacted):** `mcp_tool_called` {server, tool, decision}, `mcp_tool_result` {status, latency_ms, bytes}, `mcp_catalog_snapshot`, `mcp_server_trusted/revoked`.
- **Doctor:** `mcp_server.<id>` status + remediation; `mcp_parity` matrix (runtimes×servers) and local trusted-completion count in `--doctor --json`.
- **Structured fields:** server id, tool name, trust tier, decision (allowed/approved/denied), latency, redaction-applied flag.

## Technical Considerations

### Key Decisions
- **Decision:** Adopt `rmcp` behind an internal `McpClient` trait. **Rationale:** the protocol for free + a swappable, mockable seam. **Trade-off:** a pre-1.0 dependency. **Rejected:** hand-rolled JSON-RPC (ADR-004).
- **Decision:** `McpSupervisor` actor + command channel. **Rationale:** centralized, event-recorded lifecycle; concurrent per-connection dispatch avoids head-of-line blocking. **Rejected:** shared lock-map; per-call spawn (ADR-005).
- **Decision:** trust + pins in `.atelier/mcp-trust.json`. **Rationale:** durable, revocable, config stays read-only. **Rejected:** TOML write-back; session-only (ADR-006).
- **Decision:** read-only auto-allow = resources only; server hints informational. **Rationale:** server data is not a trust boundary. **Trade-off:** read-only *tools* prompt once pre-trust (refines CF3). **Rejected:** honor `readOnlyHint` (ADR-007).
- **Decision:** record-time redaction. **Rationale:** payloads persist full-size; display-only redaction leaks to disk. **Trade-off:** new interception at event write.

### Known Risks
- **Small-model emission breaks parity** (likely on weakest runtime) → emission spike gates the build; structured repair loop; per-runtime degrade-not-abandon.
- **`rmcp` API/version churn** → pin + trait wrapper localizes change.
- **Supervisor head-of-line blocking** → concurrent dispatch per connection, not a global call mutex.
- **Trust-file ↔ event divergence** → events are the audit trail; file is a rebuildable snapshot.
- **Prompt non-determinism from live tools** → snapshot invariant + guard test (further research: behavior on `tools/list_changed` mid-run — deferred, hot-reload is a non-goal).

## Architecture Decision Records

- [ADR-001: Broker MCP through the harness ActionRequest contract](adrs/adr-001.md) — harness-owned client, not native per-CLI MCP or a gateway.
- [ADR-002: stdio-first V1; defer HTTP + OAuth as one bundle to V1.1](adrs/adr-002.md) — transport-agnostic config; async/event-recorded connect.
- [ADR-003: Config-first MVP product surface for V1](adrs/adr-003.md) — TOML + existing approval card + doctor; defer `/mcp` commands.
- [ADR-004: Adopt the official `rmcp` Rust SDK](adrs/adr-004.md) — wrapped behind an internal `McpClient` trait.
- [ADR-005: `McpSupervisor` as a supervisor actor with a command channel](adrs/adr-005.md) — centralized lifecycle, concurrent dispatch.
- [ADR-006: Persist MCP trust and description pins in an app-managed `.atelier/` store](adrs/adr-006.md) — durable, revocable, workspace-scoped.
- [ADR-007: Read-only auto-allow via the protocol resource/tool split](adrs/adr-007.md) — server hints informational, not gating.
- [ADR-008: Deterministic MCP testing via an in-repo fake stdio server](adrs/adr-008.md) — three tiers; live tests ignored behind env var.
