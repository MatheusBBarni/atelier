# TechSpec: Rich Approval Modal + Per-Session Trust List

## Executive Summary

This feature extends atelier's **single action-enforcement point** (`validate_action_request_with_scope` in `src/actions/mod.rs`) rather than adding a parallel gate. A pure `assess_risk` produces a structured `RiskNote` (tier, `catastrophic` flag, reason, trust target); the enforcement point combines it with a `FloorPolicy` and a session **trust snapshot** carried on `ActionExecutionContext`, and returns an enriched `ActionDecision`. `execute_action_request` stamps the outcome onto `ActionResult`, so the `App` records the right events and the TUI renders a rich modal — all from the existing `AppState` snapshot. Trust is in-memory on `App`, granted via a new `ApproveAndTrust` resolution, and managed with a `/trust` command. Deny-and-continue reuses the existing `drive_and_replay` resume path.

**Primary trade-off:** centralizing floor + trust in one enforcement function (one source of truth, can't drift) costs enriching several core types and the `ActionResult` event schema (mitigated with `#[serde(default)]`). The alternative — an app-level wrapper — was rejected (ADR-003) because it splits the safety invariant across two sites.

**PRD goal → component map:** catastrophic protection even in Yolo → `assess_risk.catastrophic` + enforcement matrix; faster confident decisions → enriched `PendingApprovalView` + TUI modal; fewer risky auto-approvals → `FloorPolicy` matrix + trust never covering catastrophic; make "no" cheap → existing `drive_and_replay` + structured denial reason; phased rollout → `[approval] floor = warn|enforce`; trust visibility → `TrustStore` + `/trust` + projection events; onboarding → reused first-approval latch + Approvals help tab.

## System Architecture

### Component Overview

- **`src/actions/mod.rs` (risk + enforcement).** New `assess_risk(request, context) -> RiskNote`; a shared `normalize_command` (tilde/`$HOME` expansion) used by both classification and trust-key construction; enriched `ActionDecision`; enriched `ActionResult` (`risk`, `gate_outcome`); `ActionExecutionContext` gains `floor` + `trusted_targets`. `validate_action_request_with_scope` keeps hard checks (schema/tool/capability/path/VCS → `Denied`) then applies the floor/trust/mode matrix. `execute_action_request` translates the decision into an `ActionResult`.
- **`src/config/mod.rs` (rollout posture).** `FloorPolicy { Warn, Enforce }`, `ApprovalConfig`, `EffectiveConfig.approval`, `[approval]` raw section + merge. The catastrophic core is hard-coded (not configurable off).
- **`src/app/mod.rs` (session state + lifecycle).** `TrustStore` on `App`; `AppState.pending_approval` enriched; `ApprovalResolution`/`ApprovalSignal`/`ApprovalHandle` carry the resolution; `resolve_pending_approval` grants trust on `ApproveAndTrust` and records new events; `handle_trust_command`; the per-action `ActionExecutionContext` is built with `floor` (from config) + `trusted_targets` (snapshot).
- **`src/app/chat/projection.rs` (transcript).** Project `approval_auto_resolved`, `floor_warned`, `trust_granted`, `trust_revoked`, `trust_cleared`; enrich the `Approval` item with the tier.
- **`src/tui/mod.rs` (rendering + input).** Rich modal (tier label, resolved command, diff, affected paths, boundary, reversibility); key routing for `ApproveOnce` / `ApproveAndTrust` / `Deny` + type-to-confirm for catastrophic; Approvals/Keys help-tab copy; risk-tier theme tokens.
- **`src/slash_commands.rs`.** Register `/trust` as an `AppCommand`.
- **`src/history/mod.rs`.** Reuse the existing first-approval latch (tier-aware copy); **no** new persistence (trust is in-memory).

**Data flow:** agent `ActionRequest` → `App` builds `ActionExecutionContext` (mode, workspace, `floor`, trust snapshot) → `execute_action_request` → `validate_action_request_with_scope` (hard checks → `assess_risk` → matrix) → `ActionDecision` → `ActionResult{status, risk, gate_outcome}` → `App` records event(s); if `ApprovalRequired`, builds `PendingApprovalView` → `AppState` snapshot → TUI modal → keypress → `ApprovalResolution` → `resolve_pending_approval` (grant if `ApproveAndTrust`; re-run; deny → resume) → `drive_and_replay`.

## Implementation Design

### Core Interfaces

```rust
// src/actions/mod.rs
pub enum RiskTier { Low, Medium, High }

pub struct RiskNote {
    pub tier: RiskTier,
    pub catastrophic: bool,            // always prompt; ignores Yolo; never trustable
    pub reason: String,                // one-line plain-language rationale
    pub target: Option<TrustTarget>,   // exact trust key; None when catastrophic/untrustable
}

pub enum TrustTarget {
    Command(String),                   // exact, normalized resolved command
    WritePath(PathBuf),                // exact target path for WriteFile/ApplyPatch
}
```

```rust
// src/actions/mod.rs — enforcement output (enriched)
pub enum ActionDecision {
    Allowed,
    AllowedByTrust(TrustTarget),
    AllowedWithWarning(RiskNote),      // gray-area under Yolo + Warn
    RequiresApproval(RiskNote),        // was RequiresApproval(String)
    Denied(String),
}
pub enum GateOutcome { Normal, AutoApprovedByTrust, WarnedAllowed, ApprovalRequired }
// ActionExecutionContext gains:
//   pub floor: FloorPolicy,
//   pub trusted_targets: Arc<HashSet<TrustTarget>>,
// ActionResult gains (#[serde(default)]):
//   pub risk: Option<RiskNote>,
//   pub gate_outcome: GateOutcome,
```

```rust
// src/app/mod.rs — approval resolution replaces the bare bool
pub enum ApprovalResolution { Deny, ApproveOnce, ApproveAndTrust }
pub struct ApprovalSignal { pub sequence: u64, pub resolution: ApprovalResolution }

impl ApprovalHandle {
    pub fn resolve(&self, resolution: ApprovalResolution) { /* bump sequence, send */ }
}
```

```rust
// src/app/mod.rs — session trust (in-memory, never persisted)
#[derive(Default)]
pub struct TrustStore { entries: Vec<TrustTarget> } // Vec preserves /trust listing order

impl TrustStore {
    pub fn contains(&self, t: &TrustTarget) -> bool { self.entries.iter().any(|e| e == t) }
    pub fn grant(&mut self, t: TrustTarget) -> bool { /* insert if absent */ }
    pub fn revoke_index(&mut self, one_based: usize) -> Option<TrustTarget> { /* ... */ }
    pub fn clear(&mut self) { self.entries.clear() }
    pub fn snapshot(&self) -> Arc<HashSet<TrustTarget>> { /* clone into set */ }
}
```

### Data Models

- **`FloorPolicy`** (`src/config/mod.rs`): `enum { Warn, Enforce }`, serde snake_case, default `Warn`. **`ApprovalConfig { floor: FloorPolicy }`** → `EffectiveConfig.approval`; `RawConfig.approval: Option<RawApprovalConfig>` merged in the existing defaults → home → local → CLI chain.
- **Enriched `PendingApprovalView`** (`src/app/mod.rs`): existing fields plus `tier: RiskTier`, `catastrophic: bool`, `reason: String`, `resolved_command: Option<String>`, `diff: Option<String>` (capped preview), `affected_paths: Vec<String>`, `boundary_crossed: Option<String>`, `reversible: Option<bool>`, `trust_target: Option<TrustTarget>` (labels the approve-and-trust option; `None` hides it).
- **New event payloads** (via `record_event`): `approval_auto_resolved { action_id, target }`, `floor_warned { action_id, tier, reason }`, `trust_granted { target }`, `trust_revoked { target }`, `trust_cleared { count }`. `approval_requested`/`approval_resolved` gain a `tier` field.

### Command & Signal Surface

*(This is a terminal app — no HTTP/DB API. The external surface is the `/trust` command and the approval signal.)*

| Command | Behavior |
|---|---|
| `/trust` | List active session-trusted entries (numbered, with "this session only" scope) |
| `/trust revoke <n>` | Revoke the entry at 1-based index `n` from the last listing |
| `/trust clear` | Remove all trust entries |

Approval keypress → `ApprovalResolution`: a default key → `ApproveOnce`; a distinct key → `ApproveAndTrust` (hidden when `trust_target` is `None`); `n`/other → `Deny`. High-tier prompts ignore Enter-to-approve; the catastrophic core additionally requires type-to-confirm (PRD §User Experience).

## Integration Points

Not applicable — the feature is local-only with no external services or network calls (PRD §High-Level Technical Constraints). All metrics derive from the existing local event log.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/actions/mod.rs` | modified | `assess_risk`, normalization, enriched `ActionDecision`/`ActionResult`/context, matrix in enforcement point. Med risk (core path) | Add types + extend enforcement; keep `assess_risk`/`match_trust` as helpers |
| `src/config/mod.rs` | modified | `[approval]`/`FloorPolicy`/`ApprovalConfig` + merge. Low risk | Add section + defaults + merge + `--print-config` coverage |
| `src/app/mod.rs` | modified | `TrustStore`, resolution enum, `resolve_pending_approval` grant, event recording, context build. Med risk (signal API change) | Implement; update `ApprovalHandle` callers |
| `src/app/chat/projection.rs` | modified | Project new events; tier on `Approval` item. Low risk | Add dispatch arms |
| `src/tui/mod.rs` | modified | Rich modal, key routing, type-to-confirm, help tabs, theme tokens. Med risk (key precedence) | Extend approval branch; keep colors in `theme.rs` |
| `src/slash_commands.rs` | modified | Register `/trust`. Low risk | Add catalog entry |
| `src/history/mod.rs` | modified | Tier-aware first-run copy; no new persistence. Low risk | Update explainer text only |
| Event log schema | modified | New event kinds + fields on `ActionResult`/approval events. Low risk | `#[serde(default)]`; projection tolerates missing fields |

## Testing Approach

### Unit Tests
- **Catastrophic set (table-driven)** + **adversarial-variant suite**: `rm -rf ~`, `rm -rf $HOME`, `${HOME}`, quoting/spacing variants, force-push, secret reads, fetch-and-run — assert each classifies `catastrophic` and exposes no trust target.
- **Normalization parity**: the single `normalize_command` used by both classification and `TrustTarget::Command` — assert a trusted command still matches after re-normalization (guards the ADR-004 drift risk).
- **Enforcement matrix**: for each (tier × `ApprovalMode` × `FloorPolicy`), assert the resulting `ActionDecision` (e.g., gray-area + Yolo + Warn → `AllowedWithWarning`; catastrophic + Yolo → `RequiresApproval`; trusted non-catastrophic → `AllowedByTrust`).
- **TrustStore** grant/contains/revoke-by-index/clear; **config merge** for `[approval] floor`.

### Integration Tests (`tests/` via `FakeRuntime`)
Drive real runs with control phrases that emit specific `ActionRequest`s, asserting end-to-end: (1) trust auto-approves the identical repeat (no second `pending_approval`); (2) gray-area under Yolo+Warn emits a `floor_warned` chat item but runs; (3) a catastrophic action prompts even under Yolo; (4) deny → the agent receives the denied result and the run resumes (deny-and-continue); (5) `/trust` lists then `revoke` re-arms the prompt.

## Development Sequencing

### Build Order
1. **Risk classification + normalization** (`assess_risk`, `RiskTier`, `RiskNote`, `TrustTarget`, shared `normalize_command`) in `src/actions/mod.rs` — no dependencies.
2. **Config** (`FloorPolicy`, `ApprovalConfig`, `[approval]` merge) in `src/config/mod.rs` — no dependencies (parallel to step 1).
3. **Enriched enforcement** (`ActionDecision`/`ActionResult`/`ActionExecutionContext` fields; matrix in `validate_action_request_with_scope`; translation in `execute_action_request`) — depends on 1, 2.
4. **TrustStore + context build** (`TrustStore` on `App`; build per-action context with `floor` from step 2 + trust snapshot) — depends on 2, 3.
5. **Resolution + grant + events** (`ApprovalResolution`/`ApprovalSignal`/`ApprovalHandle`; `resolve_pending_approval` grants on `ApproveAndTrust`; record new events) — depends on 3, 4.
6. **Chat projection** (new event arms; tier on `Approval` item) — depends on 5.
7. **TUI modal + key routing** (rich fields, `ApproveOnce`/`ApproveAndTrust`/`Deny`, type-to-confirm, theme tokens) — depends on 5, 6.
8. **`/trust` command** (`slash_commands` entry + `handle_trust_command`) — depends on 4, 6.
9. **Onboarding + help** (tier-aware first-run copy; Approvals/Keys tabs) — depends on 7.
10. **Test suites** (unit + adversarial + `FakeRuntime` e2e) — written alongside each step; the e2e suite completes after 7–8.

### Technical Dependencies
None external. All work is within existing crates/files; no new packages, directories, infrastructure, or services.

## Monitoring and Observability

All KPIs (PRD §Success Metrics) derive from the existing `.atelier/sessions/<id>/events.jsonl` — no new telemetry:
- **Catastrophic coverage / risky-auto-approval reduction:** correlate executed actions (`action_completed`) classified high/catastrophic against a preceding `approval_requested`.
- **Repeat-prompt collapse:** ratio of `approval_auto_resolved` to repeats matching an existing `trust_granted` target.
- **Decision latency:** `approval_resolved.ts − approval_requested.ts` for first-seen targets.
- **Trust adoption:** sessions with ≥1 `trust_granted` ÷ sessions with ≥1 `approval_requested`.
- **Warn-signal health:** `floor_warned` counts and `floor=enforce` opt-in among sessions that emitted ≥1 `floor_warned`.

Structured fields: every new event carries `action_id`/`target`/`tier` as applicable; `--doctor` gains an approval-config check reporting `approval_mode` and `floor`.

## Technical Considerations

### Key Decisions
- **Single enforcement point, enriched** (ADR-003): one source of truth for floor + trust; trade-off is enriching core types + event schema. Rejected an app-level wrapper (two enforcement sites).
- **Conservative normalization** (ADR-003): tilde + `$HOME` only; no shell emulation. Rejected literal/tilde-only (misses `$HOME` deletion) and full expansion (a shell reimplementation).
- **Exact-target, in-memory, command+path trust** (ADR-004): bounded and ephemeral; trade-off is the `bool → ApprovalResolution` signal change. Rejected commands-only (misses edit fatigue) and persisted trust ("approve once, exploit forever").
- **Phased two-tier floor** (ADR-002): catastrophic enforces immediately, gray-area warn-only by default.

### Known Risks
- **Normalization drift** between classification and trust keys → silent match misses. *Mitigation:* one shared `normalize_command`, parity test.
- **Event-schema evolution** (`ActionResult` is serialized) → old sessions must still project. *Mitigation:* `#[serde(default)]`, projection tolerates missing fields.
- **Catastrophic-set completeness** (an escape runs under Yolo+Warn). *Mitigation:* small/high-precision set + adversarial suite; fail-closed gray-area in `enforce`.
- **Modal bloat** from large diffs over the snapshot channel. *Mitigation:* cap the `diff`/`resolved_command` preview length app-side.
- **Key-routing regressions** in the approval branch. *Mitigation:* keep precedence order; reuse the clarification options-list pattern; tests for each resolution key.

## Architecture Decision Records

- [ADR-001: V1 scope — fail-closed destructive floor, decision-support modal, and minimal floor-anchored session trust](adrs/adr-001.md) — Floor as a fail-closed allowlist, rich modal, single exact-target session-trust scope.
- [ADR-002: Phased floor rollout with a non-bypassable catastrophic core](adrs/adr-002.md) — Catastrophic enforces even in Yolo; gray-area ships warn-only, flips to enforce later.
- [ADR-003: Enforce floor + trust at the single enforcement point via a structured risk assessment](adrs/adr-003.md) — Extend `validate_action_request_with_scope` with floor posture + trust snapshot; enriched `ActionDecision`/`ActionResult`; tilde+`$HOME` normalization.
- [ADR-004: In-memory exact-target session trust, keyed by command/path, never covering the catastrophic core](adrs/adr-004.md) — `TrustStore` on `App`, `ApproveAndTrust` resolution, `/trust` management, audited via events.
