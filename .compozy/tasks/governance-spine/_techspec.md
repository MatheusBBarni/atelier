# Technical Specification: Governance Spine

## Executive Summary

This spec formalizes a governance-pause surface that both sibling packets already approximate through the clarification channel, and adds the spine's net-new capability — a single-agent turn-1 early-abort. A new concrete **`GovernanceDecisionView`** (held in one unified `App.pending_governance_decision`) rides the existing clarification pause/resume transport (`RunDriveContext` capture + `drive_and_replay`); the early-abort populates it from the orchestrator's `reason`/`plan`/`agent` plus the run's workspace write-roots. Outcome metrics are derived as a labeled proxy over existing history events and surfaced through `--doctor --json`. The spine **references, never reimplements** the siblings' types (`RiskNote`, `ExecutionGraph`); they migrate onto the shared decision in a later phase.

**Primary trade-off:** building a *generic shared* decision type and pending state up front (for a single V1 consumer) costs more than a one-off early-abort modal — but it's what prevents the three governance surfaces from diverging and lets the siblings converge without a second refactor. We accept that up-front generality in exchange for the unification that is the spine's reason to exist.

## System Architecture

### Component Overview

- **`src/governance.rs` (new, single file)** — the shared contract: `GovernanceDecisionView`, `GovernancePlanView`/`Step`, `GovernanceAnswer`, `GovernanceKind`. Referenced by app, projection, doctor, and (later) the siblings. Boundary: data + pure helpers only; no run state.
- **`src/app/mod.rs` (modified)** — owns `pending_governance_decision`, `resolve_pending_governance_decision`, and the early-abort gate in `drive_run_inner`. Mirrors the existing `pending_clarification` lifecycle.
- **`src/orchestrator/mod.rs` (modified)** — a light prompt instruction so `reason`/`plan` reliably carry an interpreted-goal + approach on the first turn.
- **`src/app/chat/` (modified)** — `ChatItemKind::GovernanceDecision`, `ChatLifecycleKey::GovernanceDecision`, and one projection arm.
- **`src/tui/mod.rs` (modified)** — render the decision card; a key-routing slot between clarification and approval.
- **`src/doctor/mod.rs` (modified)** — the outcome proxy + calibration metrics as a `DoctorCheck`.

**PRD → component mapping:** coherent governance UX → `GovernanceDecisionView` + shared projection arm (CF1/CF2); single-agent gap → early-abort gate (CF3); honest measurement → doctor proxy + calibration (CF4); no disruption → reference-not-reimplement + the conformance contract, phased (CF5).

**Data flow (early-abort):** first orchestrator turn → `Decision(SingleAgent)` with write capability → gate builds a `GovernanceDecisionView` → `pending_governance_decision` (`WaitingForUser`) → projected as a decision card → Accept resumes via `drive_and_replay`; Reject appends an optional redirect and re-drives. Nothing is written before the decision.

## Implementation Design

### Core Interfaces

The shared decision (consumers populate it; the early-abort is the V1 consumer):

```rust
// src/governance.rs
pub enum GovernanceKind { EarlyAbort /* PlanApproval, ActionApproval added when siblings migrate */ }

pub struct GovernanceDecisionView {
    pub run_id: String,
    pub decision_id: String,
    pub kind: GovernanceKind,
    pub title: String,                 // e.g. "Confirm intent before this run edits files"
    pub intent: String,                // interpreted goal (from decision.reason)
    pub approach: Vec<String>,         // approach bullets (from decision.plan)
    pub agent: Option<String>,
    pub write_scope: Vec<String>,      // workspace write-roots this run may touch
    pub risk_label: String,            // plain-language; words, not color
    pub plan: Option<GovernancePlanView>, // structured payload; DAG fills this later
}
```

The minimal shared plan/intent legibility model (one step for the echo, N for a future DAG):

```rust
// src/governance.rs
pub struct GovernancePlanView { pub steps: Vec<GovernancePlanStep>, pub edges: Vec<(String, String)> }
pub struct GovernancePlanStep { pub id: String, pub agent: String, pub label: String, pub write_scope: Vec<String> }
```

The unified pending state + resolve, mirroring `resolve_pending_clarification`:

```rust
// src/app/mod.rs
struct PendingGovernanceDecision { run: RunDriveContext, view: GovernanceDecisionView }

pub enum GovernanceAnswer { Accept, Reject { redirect: Option<String> } }

pub async fn resolve_pending_governance_decision(&mut self, answer: GovernanceAnswer) -> Result<()>;
```

The early-abort trigger (pure predicate next to the drive loop):

```rust
// src/app/mod.rs
fn early_abort_triggers(run: &RunDriveContext, d: &OrchestratorDecision) -> bool {
    d.status == DecisionStatus::Continue
        && run.step_count == 0 && run.previous_results.is_empty() && run.subtask.is_none()
        && matches!(d.normalized_next_step(), Ok(Some(DecisionNextStep::SingleAgent(_))))
        && d.required_capabilities.iter().any(|c| c.is_write())   // complexity signal
}
```

### Data Models

- **`GovernanceDecisionView` / `GovernancePlanView`** — above; serializable, replay-safe (frozen into the event).
- **`GovernanceAnswer`** — `Accept` | `Reject { redirect }`.
- **New event kinds (free-form `kind`):** `governance_decision_requested` (payload = the view), `governance_decision_resolved` (payload = `{ decision_id, outcome: accept|reject, redirect? }`).
- **`ChatItemKind::GovernanceDecision`, `ChatLifecycleKey::GovernanceDecision { run_id, decision_id }`** — new variants.
- **Metric proxy (doctor `context` JSON):** `{ trusted_outcome_rate_proxy, governed_runs, kept, early_abort_catch_rate, intervention_rate, gate_precision }`.

### Action & Config Surface

(No HTTP API.) A feature flag `features.governance_early_abort` (default off, mirrors `parallel_step_groups`); the two `governance_decision_*` events; the orchestrator-prompt instruction; the `--doctor --json` metric check.

## Integration Points

- **Clarification transport (reused):** the early-abort captures `RunDriveContext` and resumes via `drive_and_replay`, exactly as clarification does — no new resume machinery.
- **Sibling packets (referenced, not reimplemented):** the spine ships the shared `GovernanceDecisionView` + projection; `approval-trust-list`'s `RiskNote`-backed approval and `subtask-dag-execution`'s `ExecutionGraph` plan view migrate to populate it in Phase 2. The spine's V1 must not modify either sibling's shipped code.

## Impact Analysis

| Component | Impact | Description and Risk | Required Action |
|-----------|--------|----------------------|-----------------|
| `src/governance.rs` | new | Shared decision + plan-view types. Low risk (data only) | Add single-file module |
| `src/app/mod.rs` | modified | `pending_governance_decision` + resolve + drive-loop gate. **Med-high risk** (touches the run loop) | Mirror clarification; gate behind flag |
| `src/orchestrator/mod.rs` | modified | Prompt instruction for turn-1 goal/approach. Med risk (affects all runs) | Additive instruction; test contract adherence |
| `src/app/chat/mod.rs` | modified | New `ChatItemKind` + `ChatLifecycleKey` variants. Low risk (additive) | Add variants |
| `src/app/chat/projection.rs` | modified | One projection arm for the decision. Low risk | Mirror `apply_clarification_requested` |
| `src/tui/mod.rs` | modified | Decision-card render + key-routing slot. Med risk (precedence) | Insert between clarification and approval |
| `src/doctor/mod.rs` | modified | Outcome proxy + calibration `DoctorCheck`. Low risk | Derive from events |
| `src/config/mod.rs` | modified | `features.governance_early_abort` flag. Low risk | Mirror existing flag |

## Testing Approach

### Unit Tests
- `early_abort_triggers`: true for first-turn single-agent write decision; false for read-only, for non-first turn, for subtasks, for parallel/DAG steps.
- `resolve_pending_governance_decision`: Accept resumes; Reject with redirect re-drives with the redirect appended; Reject without redirect aborts.
- Metric proxy: a completed governed run with no corrective re-prompt counts kept; an abort-after-accept and an early-abort reject count against; intervention-rate band computed.
- Projection arm builds a `GovernanceDecision` chat item with intent/approach/write-scope/risk label.

### Integration Tests
- `FakeRuntime` first-turn single-agent **write** run → early-abort pauses; **Accept** → run proceeds and writes; **Reject (redirect)** → orchestrator re-drives.
- A **read-only** first-turn run → does **not** pause (complexity gate).
- With the flag **off** → no early-abort fires.
- `--doctor --json` after a governed session → includes the proxy + calibration figures.

## Development Sequencing

### Build Order
1. **`src/governance.rs` types + `governance_decision_*` events + `ChatItemKind`/`ChatLifecycleKey` variants** — no dependencies.
2. **`pending_governance_decision` + `resolve_pending_governance_decision`** (clarification-transport reuse) — depends on 1.
3. **Projection arm + TUI render + key-routing slot** — depends on 1, 2.
4. **Early-abort gate in `drive_run_inner`** (predicate + build view + pause) + `features.governance_early_abort` flag — depends on 2, 3.
5. **Orchestrator-prompt nudge** for turn-1 interpreted-goal/approach — depends on 4 (the echo consumes it).
6. **Outcome proxy + calibration metrics in `--doctor --json`** — depends on 4 (needs governance events).
7. **Sibling-conformance contract** (documented interface; migration is a later phase) — depends on 1.

### Technical Dependencies
- None external. Reuses the clarification transport, the event store, and the doctor surface.
- The orchestrator-prompt change is additive and affects all runs; validate contract adherence on the smallest configured model.

## Monitoring and Observability

- **Events (durable):** `governance_decision_requested` (intent/approach/scope/risk), `governance_decision_resolved` (accept/reject/redirect).
- **`--doctor --json`:** the Trusted Outcome **proxy** (clearly labeled) plus the exact calibration metrics — intervention rate (dual-alarm band), early-abort catch rate, gate precision. Local-only.

## Technical Considerations

### Key Decisions
- **Decision:** One concrete `GovernanceDecisionView` in a unified `pending_governance_decision`. **Rationale:** matches the codebase's concrete-pending style; unifies the three surfaces. **Trade-off:** generic type for one V1 consumer. **Rejected:** trait; per-consumer states; overloading clarification (ADR-003).
- **Decision:** Early-abort gates on first-turn single-agent + write-capability; echo from `reason`/`plan` + workspace write-roots; prompt-nudge for quality. **Rationale:** reuse + no schema change. **Rejected:** structured `goal_statement`/`file_scope` fields; raw prose (ADR-004).
- **Decision:** Outcome metric as an event-derived, clearly-labeled proxy. **Rationale:** ships the North Star with no new instrumentation. **Rejected:** first-class revert events; deferring the metric (ADR-005).

### Known Risks
- **Echo quality depends on prompt adherence** (esp. weak models) → degrade gracefully; never block on echo quality; the prompt-nudge is best-effort.
- **Complexity heuristic is coarse** (write-capability presence, not write-scope size) → acceptable for V1; refine from data.
- **Proxy noise** (corrective re-prompt is heuristic) → label it a proxy; decisions lean on the exact calibration metrics.
- **Sibling migration coordination** → V1 only defines + documents the contract; no sibling code changes until each adopts on its own schedule.

## Architecture Decision Records

- [ADR-001: Reframe as a governance spine consumed by the sibling packets](adrs/adr-001.md) — own the shared contract; siblings consume.
- [ADR-002: V1 product shape — shared contract + early-abort, phased sibling migration](adrs/adr-002.md) — reference, don't reimplement.
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](adrs/adr-003.md) — concrete shared type + one pending surface.
- [ADR-004: Single-agent turn-1 early-abort mechanism](adrs/adr-004.md) — drive-loop gate, prompt-nudged echo, capability-based complexity.
- [ADR-005: Outcome metric as an event-derived proxy](adrs/adr-005.md) — proxy + exact calibration via `--doctor --json`.
