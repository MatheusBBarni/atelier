# TechSpec: Best-of-N Cross-Runtime Race (`/race`)

## Executive Summary

`/race` adds a dedicated, user-invoked workflow that runs the same instruction across N≤3 distinct runtimes, grades each attempt with the project's existing test oracle, deterministically selects the winner, and promotes it through the existing approval gate — while accumulating per-runtime win-rates that route future race rosters. The design is **reuse-heavy**: it borrows the oracle (`derive_grade_verdict`), the approval/scope fence (`validate_action_request_with_scope`), the in-memory patcher (`apply_unified_diff`), and the council/parallel-group lifecycle plumbing. The only genuinely new subsystems are a per-attempt **scratch overlay** (writes-redirect, reads fall through), a **dedicated runner** (`run_race_workflow`), a **verdict ledger**, and a **race projection** for the live panes + verdict card.

**Primary trade-off:** we add a *fourth* step-execution path and a branch in the hot `resolve_action_path` choke point, rather than bending the DAG/parallel schedulers to a shape they don't fit. We accept some spawn-plumbing similarity to `run_parallel_group` and overlay-correctness risk in exchange for conceptual clarity, zero regression to existing schedulers, and a winner that promotes as "just another `ApplyPatch`."

## System Architecture

### Component Overview

| Component | Responsibility | Reuse / New |
|---|---|---|
| **Command dispatch** | `/race <instruction>` parsed in `App::submit_prompt`; opens a `RunDriveContext` with `RaceRunContext`; announces N/roster/cost | New (mirrors `/workflow`) |
| **Roster router** | Pure fn: `(task_signature, ledger) → ordered roster`; cold-start → default roster | New (ADR-007) |
| **`run_race_workflow`** | Spawns N attempts concurrently, grades, selects, narrates, promotes; appends `RunStepResult::Race` | New (ADR-005; lifecycle from `run_council_workflow`, spawn from `run_parallel_group`) |
| **Attempt isolation** | `ActionScope::AttemptScope`; overlay in `resolve_action_path`; scratch lifecycle | New (ADR-006) |
| **Oracle grading** | Per-attempt compile/test/lint over the overlay → `GraderVerdict` | Reuse `derive_grade_verdict` |
| **Selector + judge** | Deterministic pick among passing attempts; reviewer-profile judge narrates + tie-breaks | New selector; judge reuses `council_member_agent` dispatch (ADR-008) |
| **Promotion** | Winner's scratch→real diff, re-derived scope, replay as `ApplyPatch` through approval gate | Reuse fence + patcher (ADR-006) |
| **Telemetry** | Per-run `ensemble_attempt_verdict` events + append to `.atelier/race/verdicts.jsonl` | Reuse `record_event`; new ledger |
| **Projection** | Live multi-pane + collapsed verdict card | New `ChatItemKind::Race` arm |
| **Config** | `[ensemble]` block + `features.ensemble` | New (mirrors `GradingConfig`/`CouncilConfig`) |

**Data flow:** `/race` → router builds roster from the ledger → runner spawns N attempts, each writing into its `AttemptScope` overlay → each attempt graded by the oracle over its overlay → deterministic selector ranks passing attempts → judge step writes rationale (and breaks ties only when undecided) → winner's diff re-derived and sent through the approval gate → on approve, applied to the real tree → verdict events + ledger records written → projection renders panes → card.

## Implementation Design

### Core Interfaces

```rust
// New ActionScope variant — the only new write-safety primitive (ADR-006).
pub enum ActionScope {
    Unrestricted,
    ParallelFileScope(ParallelFileScope),
    AttemptScope(AttemptScope),            // per-attempt overlay
}

pub struct AttemptScope {
    pub attempt_id: String,
    pub scratch_dir: PathBuf,              // .atelier/race/<run_id>/<attempt_id>/
    pub read_roots: Vec<String>,          // real-tree roots that fall through
}
```

```rust
// Workflow result variant appended to run.previous_results (ADR-005).
pub enum RunStepResult {
    Agent { result: AgentResult },
    ParallelGroup { result: ParallelGroupResult },
    Dag { result: ExecutionGraphResult },
    Race { result: RaceResult },          // new
}

pub struct RaceResult {
    pub status: RaceStatus,               // Promoted | AllFailed | LowConfidence | Cancelled
    pub task_signature: TaskSignature,
    pub attempts: Vec<RaceAttempt>,
    pub winner: Option<String>,           // attempt_id
    pub rationale: Option<String>,        // judge narration
    pub low_confidence: bool,             // no-oracle / tie
    pub changed_files: Vec<String>,
}
```

```rust
pub struct RaceAttempt {
    pub attempt_id: String,
    pub runtime: String,
    pub model: String,
    pub verdict: GraderVerdict,           // reused oracle type
    pub diff_summary: String,
}

// Deterministic selection — pure, testable (ADR-008).
// Returns (winner, ordered_survivors, decisive). When !decisive, the judge breaks the tie.
fn select_winner(attempts: &[RaceAttempt]) -> Option<(String, Vec<String>, bool)>;
```

```rust
// Pure roster routing over the ledger (ADR-004/007).
fn route_roster(
    sig: &TaskSignature,
    ledger: &VerdictLedger,
    default_roster: &[RosterMember],
    min_samples: u32,
    n: u32,
) -> Vec<RosterMember>;                   // < min_samples → default_roster
```

### Data Models

- **`TaskSignature` (v1):** `{ sig_version: u32, primary_language: String, change_kind: ChangeKind }`, `ChangeKind ∈ {Feature, Refactor, Bugfix, Test, Docs, Config}`. Derived deterministically from the instruction + target files.
- **Verdict ledger record** (`.atelier/race/verdicts.jsonl`, append-only, cross-session): `{ schema_version, timestamp, run_id, task_signature, runtime, model, oracle_outcome, exit_code, won, cost_tokens }`.
- **`ensemble_attempt_verdict` event** (per-run, transcript): same payload, keyed by `run_id` for projection.
- **`EnsembleConfig`** (`[ensemble]`): `{ enabled: bool (default false), max_attempts: u32 (default 3, cap 3), timeout_seconds: u64, default_preset: String, presets: BTreeMap<String, BTreeMap<String, RosterMember>>, min_route_samples: u32 }`. `RosterMember` mirrors `CouncilMemberProfile` (runtime/model/effort).
- **`Features.ensemble: bool`** + `RawFeatures.ensemble: Option<bool>` (merge per `config/mod.rs:1331`).

### Command & Config Surface

*(This is a terminal app; the template's "API Endpoints" maps to the command + config surface.)*

- **Command:** `/race <instruction>` — `SlashCommandKind::AppCommand` in `slash_commands.rs` (amend the frozen-V1 catalog ADR note + `FIXED_V1_LABELS`). Rejects empty input; if `<2` runtimes configured or `features.ensemble == false`, returns a clear message (no silent degrade).
- **Start line:** `Racing N runtimes (<roster>) · est. ~Nx cost`.
- **Read-back:** `/provider:status` gains a win-rate-by-task-type section sourced from the ledger; shows "still learning" below `min_route_samples`.
- **Discoverability:** `--doctor` / `/config` report `[ensemble]` enabled state and roster.

## Integration Points

- **Runtimes:** each attempt is a normal agent step on a distinct configured runtime — reuses `execute_runtime_step_streaming` and the model-fallback chain; no new runtime code.
- **Oracle:** `derive_grade_verdict` run per attempt over its overlay; respects existing `[grading]` command configuration.
- **Approval gate:** the winner's `ApplyPatch` flows through `validate_action_request_with_scope` + governance-spine floor unchanged.
- **History:** `record_event` for transcript events; new append-only ledger for the cross-session corpus.

## Impact Analysis

| Component | Impact | Description / Risk | Required Action |
|---|---|---|---|
| `actions::ActionScope` + `resolve_action_path` | modified | New `AttemptScope` variant + overlay read/write resolution on a hot path — **medium risk** (read-after-write correctness) | Add variant; branch resolution; thorough tests |
| `validate_action_scope` | modified | Handle `AttemptScope` (writes confined to scratch) — low risk | Add match arm |
| `app::submit_prompt` dispatch | modified | New `/race` command branch | Add handler |
| `app` run loop | new | `run_race_workflow` + `RaceRunContext` | New runner (reuse council/parallel plumbing) |
| `orchestrator::RunStepResult` | modified | New `Race` variant — serialization round-trip | Add variant + serde |
| `app::chat` projection | modified | `ChatItemKind::Race` + live panes/verdict card | New projection arm |
| `config` | modified | `[ensemble]` block + `features.ensemble` | Add structs, defaults, merge |
| `slash_commands` | modified | `/race` catalog entry (frozen-V1 ADR) | Add spec + amend ADR/test |
| `.atelier/race/` ledger + scratch | new | New on-disk artifacts — cleanup/leak risk | Lifecycle guards + startup sweep |
| `provider:status` | modified | Win-rate read-back section | Add rendering |

## Testing Approach

### Unit Tests

- **Overlay resolution:** write-redirect to scratch; read-after-write sees scratch; unrelated read falls through; deletion/rename tombstones reflected.
- **`select_winner`:** all-pass tie → `decisive=false`; one pass → that winner; all-fail → `None`; objective tiebreak ordering.
- **`route_roster`:** below `min_samples` → default roster; above → win-rate order; unknown signature → default.
- **`TaskSignature` derivation:** deterministic, versioned, stable buckets.
- **Promotion scope re-derivation:** scratch write-set → `ParallelFileScope`; out-of-scope change rejected (fail-closed).
- **Config:** `[ensemble]` defaults, `max_attempts` cap at 3, merge precedence.

### Integration Tests

- **End-to-end via the `fake` runtime:** drive `/race` with control-phrase attempts (one passes, one fails, one errors); assert oracle disqualification, deterministic winner, diff-replay promotion through the approval gate, and `RaceResult` shape. Reuse the `fake` runtime harness used by app/orchestrator tests.
- **All-fail path:** every attempt fails the oracle → promote nothing, surface failures, retry/abort routing.
- **No-oracle path:** no canonical verification command → judge tie-break + `low_confidence` banner.
- **Ledger round-trip:** verdicts written → `route_roster` reads them in a later run → roster reflects history.
- **Thin-fleet guard:** `<2` runtimes → clear refusal, no race.

## Development Sequencing

### Build Order

1. **`EnsembleConfig` + `features.ensemble`** (config structs, defaults, merge) — no dependencies.
2. **`AttemptScope` + overlay `resolve_action_path` + scratch-lifecycle module** — depends on 1 (scratch path config).
3. **Verdict ledger writer + record + `TaskSignature` derivation** — depends on 1.
4. **`route_roster` pure fn** (ledger → ordered roster, cold-start) — depends on 3.
5. **`select_winner` pure fn** (over `GraderVerdict`) — depends on 2 (consumes attempt results), reuses oracle types.
6. **`run_race_workflow`** (spawn N attempts in `AttemptScope`, grade each via oracle over overlay, call selector) — depends on 2, 4, 5.
7. **Judge narration step** (reviewer profile, independent runtime; tie-break when `!decisive`) — depends on 6.
8. **Promotion** (winner scratch→real diff, re-derived scope, `ApplyPatch` through approval gate) — depends on 2, 6.
9. **`RunStepResult::Race` + projection** (live panes + verdict card, low-confidence banner) — depends on 6.
10. **`/race` command dispatch + slash catalog entry** (cost announce, thin-fleet guard) — depends on 6.
11. **Read-back in `/provider:status`** — depends on 3, 4.
12. **All-fail / no-oracle UX wiring** (retry/abort, banner) — depends on 7, 9.

### Technical Dependencies

- Relies on shipped siblings: `self-grading-retry-loop` (oracle), `governance-spine`/`approval-trust-list` (approval gate), and the `[grading]` config. No external infra.

## Monitoring and Observability

- **Events:** `race_started` (roster, N, est. cost), `ensemble_attempt_verdict` (per attempt), `race_selected` (winner, decisive, low_confidence), `race_promoted` / `race_all_failed`.
- **Ledger:** `.atelier/race/verdicts.jsonl` is both data and audit trail.
- **Metrics** (derivable): external-check win-rate lift vs single-runtime; selection-vs-oracle agreement; promotion scope-escapes (must be 0); realized cost multiplier; per-attempt verdict completeness (target 100%).
- **Structured fields:** every verdict carries `run_id`, `attempt_id`, `runtime`, `task_signature`, `oracle_outcome`, `cost_tokens`.

## Technical Considerations

### Key Decisions

- **Dedicated runner over DAG reuse** — a race has no edges/scope-partitioning; the DAG scheduler's value is unused. *Trade-off:* a fourth path vs conceptual clarity + zero scheduler regression. (ADR-005)
- **Writes-redirect overlay over tree-copy/worktree** — cost scales with the change, not the repo; promotion reuses the existing patcher/fence. *Trade-off:* a hot-path resolution branch + binary/rename handling. (ADR-006)
- **Dedicated verdict ledger over session-log scan** — cross-session, compaction-proof, fast. *Trade-off:* a second persistence artifact. (ADR-007)
- **Deterministic select + judge narrates over judge-ranks** — keeps selection on objective evidence, confines bias to disclosed ties. *Trade-off:* a judge step runs for narration even when selection is deterministic. (ADR-008)

### Known Risks

- **Overlay read-after-write correctness** (medium) → scratch-first read resolution + dedicated tests.
- **Diff-replay drift / non-text edits / deletions** (medium) → tombstones, fail-closed hunk-apply, re-validate at promotion.
- **Scratch leakage on crash** (low) → RAII guards + startup sweep of stale `.atelier/race/*`.
- **Spawn-plumbing duplication with `run_parallel_group`** (low) → extract a shared concurrent-spawn helper if real.
- **Task-signature mis-bucketing dilutes routing** (medium) → coarse + versioned signature; revisit with data in Phase 2.

## Architecture Decision Records

- [ADR-001: Oracle-Selected Pick-One Over LLM-Judged Synthesize-Merge](adrs/adr-001.md) — the oracle selects; one whole attempt is promoted.
- [ADR-002: Frame Around a Learning Fleet Router; the Race Is the Data Engine](adrs/adr-002.md) — record verdicts from day one.
- [ADR-003: PRD Approach — Race-Led, Router-Active V1](adrs/adr-003.md) — ship the full race + active routing in one release.
- [ADR-004: Minimal Routing in V1 — Route the Race Roster, Never Skip the Race](adrs/adr-004.md) — V1 routing only selects competitors.
- [ADR-005: Dedicated `run_race_workflow` Runner (Not the DAG Scheduler)](adrs/adr-005.md) — a dedicated parallel-spawn workflow runner.
- [ADR-006: Writes-Redirect + Diff-Replay Isolation and Promotion](adrs/adr-006.md) — overlay scratch; winner promoted as a re-derived `ApplyPatch`.
- [ADR-007: Dedicated Verdict Ledger + Coarse Task-Type Signature for Routing](adrs/adr-007.md) — `.atelier/race/verdicts.jsonl` + versioned signature.
- [ADR-008: Deterministic Oracle-Selection with the Judge as Narrator/Tie-Breaker](adrs/adr-008.md) — deterministic pick; judge narrates and breaks only ties.
