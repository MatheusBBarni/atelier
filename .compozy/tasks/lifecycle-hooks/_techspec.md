# TechSpec — Lifecycle Hooks (V1: Observer Tier)

## Executive Summary

Lifecycle Hooks adds a new in-crate module `src/hooks/` plus a single non-blocking **tap** in the event write path and a **long-lived dispatcher task** that runs hook actions off the worker thread. Events are recorded exactly as today; the tap (after the durable append) maps the internal event kind to a stable **public event name**, resolves the cross-runtime **actor** from `active_step`, builds a **normalized payload**, and `try_send`s it onto a bounded channel. The dispatcher drains that channel and either runs the user's shell command (payload on stdin) or fires the **built-in OSC notifier**. A standalone `atelier --events follow` reuses the *same* `normalize()` to preview exactly what a hook receives.

**Primary technical trade-off:** the design accepts **best-effort delivery** (drop-on-full backpressure) and a small **enrich-at-tap lookup + public-vocabulary mapping layer** in exchange for a write path that never blocks the render/worker loop, an app-free dispatcher, and a public contract that stays decoupled from atelier's refactorable internal event kinds.

## System Architecture

### Component Overview

- **`src/hooks/` (new module)** — owns `HookPayload`, the public-event vocabulary + `normalize()`, `HooksConfig`/`HookHandler`, the dispatcher task, and the notifier backends. Single home for the feature.
- **Event tap** — inside `record_event_with_group` (`src/app/mod.rs:4232`), immediately after `append_event`: gated on "any hooks configured"; maps kind → public name; if a handler matches, resolves `actor` (`active_step.agent → self.agent(id).runtime`), calls `normalize()`, and `try_send`s the payload.
- **Dispatcher task** — spawned in `run_tui` beside `run_app_worker`; drains the bounded channel; per matched handler runs a subprocess (`Command` + `kill_on_drop` + `select!` timeout, payload on stdin — the `run_git` idiom) or invokes the notifier; records `hook_started`/`hook_completed` back through the event path.
- **Config ladder** — `RawConfig.hooks` → `apply_raw` → `EffectiveConfig.hooks`; the Local (`./atelier.toml`) layer's hooks are dropped (ADR-001 security posture).
- **CLI** — `atelier --events follow` (a clap `ValueEnum` flag, like `--codemap`) runs standalone, tails the latest session's on-disk JSONL, and emits normalized payloads via the shared `normalize()` (actor reconstructed by folding the session's own run/step events).
- **Projection** — `apply_history_event` gains `hook_started`/`hook_completed` handlers → a new `ChatItemKind::HookInvocation`.
- **Doctor** — a "Lifecycle hooks" check (handlers configured, last fired, dropped count).

**Data flow:** event recorded → durable append → **tap** (map / match / resolve actor / normalize / try_send) → bounded channel → **dispatcher** (subprocess *or* notify) → `hook_started`/`hook_completed` → projection → chat. Hook events are outside the public vocabulary, so they never re-trigger hooks (structural recursion guard).

## Implementation Design

### Core Interfaces

The normalized payload contract (what hooks receive on stdin, and what `--events follow` prints):

```rust
#[derive(Clone, Debug, Serialize)]
pub struct HookPayload {
    pub schema_version: u32,        // payload contract version
    pub event: String,             // public event name (ADR-004)
    pub time: String,              // RFC3339
    pub session_id: String,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub actor: Actor,              // uniform across runtimes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,    // e.g. file path / command
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,   // success | error | denied | ...
}

#[derive(Clone, Debug, Serialize)]
pub struct Actor { pub agent: Option<String>, pub runtime: Option<String> }
```

The shared projection — the single source of payload truth for both the live tap and `--events follow`:

```rust
/// Returns None when the event kind is not in the public vocabulary
/// (so it never triggers a hook and never prints in `--events follow`).
pub fn normalize(event: &HistoryEvent, actor: ActorCtx) -> Option<HookPayload>;

pub struct ActorCtx { pub agent: Option<String>, pub runtime: Option<String> }
```

Config types (handler array, exactly one action per handler):

```rust
#[derive(Clone, Debug, Default)]
pub struct HooksConfig {
    pub handlers: Vec<HookHandler>,
    pub notify_fallback_command: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HookHandler {
    pub on: Vec<String>,           // public event names, exact match
    pub action: HookAction,        // Notify | Command
    pub payload: PayloadDetail,    // Metadata (default) | Full
}

pub enum HookAction { Notify(NotifyConfig), Command(String) }
```

The notifier behind an injectable backend (so tests assert output without real delivery):

```rust
pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
}
// OscNotifier (default): writes OSC 9/777 to the controlling TTY.
// CommandNotifier: spawns `notify_fallback_command` with title/body.
```

### Data Models

- **`HookPayload`** — the versioned contract above; standards-aligned in spirit (who/what/when/outcome + correlation), not bound to OTel (ADR-004).
- **Public-event vocabulary** — a static map of stable public names → internal kinds (ADR-004 table: `step_started`→`agent_step_started`, `approval_required`→`approval_requested`, `file_edited`→`file_edit_applied`, …). The contract surface.
- **`hook_started` / `hook_completed` payloads** — handler index, action kind, status, duration ms, exit code, redacted stderr excerpt. Recorded for transcript transparency.
- **Persistence** — *unchanged*. No new on-disk schema; hooks read the existing `HistoryEvent` stream and config.

### API Endpoints

No HTTP. The surface is the existing flag-based CLI (`src/cli.rs`):

| Flag / surface | Behavior |
|---|---|
| `atelier --events follow` | Standalone; tails the latest session's JSONL; prints normalized `HookPayload` JSON lines; the dry-run/test harness. Exits on Ctrl-C. |
| `atelier --doctor [--json]` | Adds a "Lifecycle hooks" check: handlers configured, last-fired status, dropped-event count. |
| `atelier --init-config` | Scaffolds a commented `[[hooks.handler]]` example. |
| `atelier --print-config` | Shows the `[hooks]` section automatically via the config ladder. |
| `/config` (TUI) | Lists active hooks. |

## Integration Points

External boundaries (V1 has no network): (1) **the user's shell command** — receives the normalized JSON on **stdin only** (never interpolated into argv); stdout/stderr captured and redacted; exit code recorded. (2) **the controlling terminal** — OSC 9/777 sequences for the default notifier. (3) **an optional fallback notifier binary** named by `notify_fallback_command`. Timeouts and `kill_on_drop` bound every subprocess.

## Impact Analysis

| Component | Impact | Description & Risk | Required Action |
|---|---|---|---|
| `src/hooks/` | new | Dispatcher, `normalize`, notifier, config + payload types | Create module |
| `record_event_with_group` (`app/mod.rs:4215`) | modified | Non-blocking tap after append; low risk, gated on hooks-present | Insert actor-resolve + `try_send` |
| `App` struct (`app/mod.rs:316`) | modified | Add `hook_sender: Option<mpsc::Sender<HookPayload>>` | Add field; wire in `run_tui` |
| `run_tui` (`tui/mod.rs`) | modified | Spawn dispatcher task + bounded channel | Spawn + pass sender |
| Config (`config/mod.rs`) | modified | `RawConfig.hooks`, `apply_raw`, drop Local-layer hooks, `EffectiveConfig.hooks`, starter text | Add structs + merge |
| `Cli` / `run_cli_with` (`cli.rs`) | modified | `--events` flag + dispatch to follow reader | Add flag + handler |
| Doctor (`doctor/mod.rs`) | modified | Hooks health check | Add check |
| Projection (`chat/projection.rs`, `chat/mod.rs`) | modified | `hook_started`/`hook_completed` handlers + `ChatItemKind::HookInvocation` | Add variant + handlers |
| `history/mod.rs` | unchanged | No schema change; hooks read existing events | None |
| `redact_sensitive_text` (`runtime/mod.rs:704`) | reused | Applied to payload + hook output | Call |
| README / docs | modified | Document `[hooks]` + recipes + tmux note | Docs |

## Testing Approach

### Unit Tests
- `normalize()` — kind→public mapping; actor population; `metadata` vs `full`; returns `None` for non-public kinds (incl. `hook_*`, `runtime_stream_delta`).
- Config parse — handler array; `on` as string or list; exactly-one-action validation; **Local-layer hooks dropped** while home hooks and other local overrides survive.
- Notifier backends — assert OSC bytes (default) and constructed fallback command, via the `Notifier` trait, with no real delivery.
- Backpressure — `try_send` on a full channel drops and increments the counter.

### Integration Tests
- Drive a full run through the **`fake` runtime** (`src/runtime/fake.rs`) with a `command` hook writing a sentinel file; assert it fired with the expected normalized payload. **Cross-runtime conformance:** assert an identical payload shape for ≥2 runtime configurations (the PRD's binding proceed-criterion).
- `--events follow` over a fixture session log emits the expected normalized JSON lines.
- `hook_started`/`hook_completed` surface in the chat projection; assert hook events do **not** re-dispatch (recursion guard).
- Real cross-platform OSC delivery is a **manual support matrix** (macOS/Linux/SSH/tmux) — not CI-assertable.

## Development Sequencing

### Build Order
1. **`src/hooks/` core types** (`HookPayload`, `Actor`, `HooksConfig`, `HookHandler`, `HookAction`, public vocabulary, `normalize()`) — no dependencies.
2. **Config integration** — depends on 1: `RawConfig.hooks`, `apply_raw`, `EffectiveConfig.hooks`, drop-Local-layer logic, `starter_config_text` example (`--print-config` follows automatically).
3. **Notifier backends** — depends on 1: `Notifier` trait, `OscNotifier`, `CommandNotifier`.
4. **Dispatcher task + channel** — depends on 1, 3: drain, subprocess (`run_git` idiom) or notify, record `hook_started`/`hook_completed`, drop counter.
5. **Event tap** — depends on 1, 2, 4: actor-resolve + `normalize` + `try_send` in `record_event_with_group`; add `App.hook_sender`; spawn dispatcher in `run_tui`.
6. **Projection** — depends on 1: `apply_hook_started`/`apply_hook_completed`, `ChatItemKind::HookInvocation`.
7. **`--events follow` CLI** — depends on 1, 2: `--events` flag + standalone tail reader using `normalize()` with folded actor context.
8. **Doctor check** — depends on 2, 4: handlers configured + last-fired + dropped count.
9. **Docs & recipes** — depends on all: README `[hooks]` section, recipes (notify / audit-to-file / webhook), tmux passthrough note.

### Technical Dependencies
None external. `tokio` (process, mpsc, time) is already a dependency; OSC sequences are raw bytes; no new crates required.

## Monitoring and Observability

- `atelier --doctor`: hooks configured, last-fired timestamp/status, **dropped-event count** (the best-effort signal).
- `hook_started`/`hook_completed` events in the transcript: handler, action, duration, exit code, redacted stderr excerpt.
- `atelier --events follow` for live inspection.
- No remote telemetry (PRD constraint); all signals are local.

## Technical Considerations

### Key Decisions
- **Enrich-at-tap, off-funnel dispatch** (ADR-003) — actor resolved where `active_step` is authoritative; subprocess work off the worker thread. *Trade-off:* best-effort delivery vs a never-blocking write path. *Rejected:* inline spawn (blocks per token); threading actor through ~100 sites.
- **Handler array + normalized contract** (ADR-004) — multiple actions per event; public names decoupled from internal kinds; one `normalize()` shared with `--events follow`. *Trade-off:* a mapping layer vs refactor-safety + faithful preview. *Rejected:* per-event single table (one action only); raw internal kinds (calcifies internals); full OTel conformance (deferred).
- **OSC-native notifier + fallback command** (ADR-005) — works over SSH with zero dependency; deterministic escape hatch. *Trade-off:* atelier owns terminal edge cases vs SSH-out-of-the-box. *Rejected:* OS shell-out default (fails over SSH); OSC-only (silent where stripped); auto-detection (unreliable).

### Known Risks
- **Best-effort delivery** under event storms → bounded channel + visible dropped-count; observer hooks are best-effort by design. Tune capacity in testing.
- **Follow-path actor reconstruction** is best-effort for events before the agent is known in-stream → actor is `null`/orchestrator there; documented.
- **OSC variance / tmux stripping** → `notify_fallback_command` + documented tmux passthrough; SSH+tmux treated as a supported, documented case.
- **Secret leakage** via `full` payload or audit egress → metadata-only default, stdin-only (no argv interpolation), hardened redaction, documented leak path.

## Architecture Decision Records

- [ADR-001: V1 ships cross-runtime observer hooks with a decision-first payload contract; blocking veto deferred to V2](adrs/adr-001.md) — V1 scope, dispatcher seam, security posture.
- [ADR-002: V1 ships a thin hook dispatcher plus one built-in battery (cross-platform notifier); audit/webhook stay recipe-based](adrs/adr-002.md) — the "batteries" level.
- [ADR-003: Off-funnel hook dispatch with enrich-at-tap actor resolution](adrs/adr-003.md) — non-blocking tap, off-thread dispatcher, actor resolution.
- [ADR-004: Handler-array config schema and a normalized, versioned cross-runtime payload contract](adrs/adr-004.md) — config shape, public vocabulary, `normalize()`.
- [ADR-005: Built-in notifier — OSC-native by default, with a configurable fallback command](adrs/adr-005.md) — notification delivery.
