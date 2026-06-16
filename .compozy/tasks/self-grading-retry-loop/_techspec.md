# TechSpec: Externally-Grounded Auto-Verification Loop

## Executive Summary

After a top-level single-agent Edit step reports `Completed` with changed files, the harness force-injects
a self-contained **grading loop**: a new executor `run_grading_workflow` (modeled on `run_council_workflow`,
`src/app/mod.rs:3155`) dispatches the built-in `reviewer` to run the project's checks, derives a
**harness-owned verdict from the recorded command exit codes** (never the agent's self-attestation), and on
FAIL re-dispatches the *same* editing agent with the concrete failures — bounded by `grading.max_attempts`.
Grade/fix sub-steps run through the action-executing runtime path *without* incrementing `run.step_count`,
so they are exempt from `max_agent_steps` and cannot re-trigger grading (the same property that keeps
council members un-graded). On exhaustion the loop escalates through the existing clarification pause with
accept/retry/abort. The whole cycle renders as one evolving chat item via a new `Grade` lifecycle key.

**Primary trade-off:** determinism and faithful "re-dispatch the same agent" behavior at the cost of a new
executor and a grader LLM turn per attempt (bounded by `max_attempts`). The design deliberately keeps the
runtime agent-result contract and the orchestrator routing untouched: the verdict is derived from
`command_completed` events the harness already records, so no runtime schema-brief changes are needed.

## System Architecture

### Component Overview

| Component | Responsibility | Boundary |
|-----------|----------------|----------|
| **Grading trigger** (`run_agent_step` `AgentResult` arm, `src/app/mod.rs:3083`) | Decide whether a just-completed step qualifies for grading and invoke the executor | Gate only; no loop logic |
| **Grading executor** `run_grading_workflow` (new, `src/app/mod.rs`) | Drive grade→fix→re-grade up to `max_attempts`; emit round events; escalate on exhaustion | Owns the loop; runs sub-steps via `execute_runtime_step_with_actions` |
| **Verdict deriver** (new, `src/orchestrator/mod.rs` + helper) | Compute `GraderVerdict` from the grade sub-step's command results | Pure function of command results; no model input |
| **Canonical-check predicate** (factored from `is_default_read_only_command`, `src/actions/mod.rs:443`) | Define "a real check" (`cargo test/check/build/clippy/fmt`) | Shared with the auto-approval allowlist |
| **Escalation** (`resolve_pending_clarification`, `src/app/mod.rs:1689`) | Interpret accept/retry/abort on exhaustion via the existing pause transport | Branch gated by a grade-escalation marker |
| **Config** `GradingConfig` (new, `src/config/mod.rs`) | `enabled` (default false) + `max_attempts` (default 2) | Mirrors `Features` opt-in precedent |
| **Chat projection** (new arm, `src/app/chat/projection.rs`) | Collapse rounds into one evolving `Grade` item | Mirrors `apply_clarification_answered`, not council |

**Data flow:** Edit step `Completed` → trigger gate (Edit cap + non-empty `changed_files` + `grading.enabled`
+ top-level single-agent) → `run_grading_workflow` → grader sub-step runs canonical command → harness reads
the recorded `command_completed` exit code → `GraderVerdict` → `grade_round` event (chat) →
PASS/SKIP: conclude; FAIL: re-dispatch the producing agent with the critique, repeat → exhausted: pause
with accept/retry/abort. No external systems; all checks run locally through the existing command path.

## Implementation Design

### Core Interfaces

The verdict and its outcome (new, `src/orchestrator/mod.rs`, near `AgentResult` at `:147`). `GraderVerdict`
is **harness-constructed** and never deserialized from agent output:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GradeOutcome { Pass, Fail, Skip }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraderVerdict {
    pub outcome: GradeOutcome,
    pub command: Option<String>,   // the canonical check that decided it
    pub exit_code: Option<i64>,    // ground truth from command_completed
    pub critique: Option<String>,  // failure excerpt fed to the fixer on FAIL
}
```

The deriver is a pure function over the grade sub-step's recorded command results:

```rust
// Pass  = >=1 canonical command ran AND all canonical commands exited 0
// Fail  = >=1 canonical command ran AND any exited non-zero
// Skip  = no canonical command ran (denied / never executed / non-cargo)
fn derive_grade_verdict(commands: &[CommandOutcome]) -> GraderVerdict;

struct CommandOutcome { command: String, exit_code: Option<i64> } // from command_completed payload

fn is_canonical_verification_command(lower: &str) -> bool; // shared with is_default_read_only_command
```

The executor and its outcome (new, `src/app/mod.rs`); `GradingOutcome::Escalated` maps to
`AgentStepOutcome::Paused`, `Concluded` lets the caller return `Completed`:

```rust
enum GradingOutcome { Concluded, Escalated }

async fn run_grading_workflow(
    &mut self,
    run: &mut RunDriveContext,
    producing_agent_id: &str,        // the editing agent re-dispatched on FAIL
    changed_files: Vec<String>,
) -> Result<GradingOutcome>;
```

### Data Models

- **`GradingConfig`** (new, `src/config/mod.rs`, sibling of `Features` at `:196`) — `#[serde(default)]` +
  manual `Default` (non-zero default), with `RawGradingConfig { enabled: Option<bool>, max_attempts:
  Option<u32> }` mirror and an `apply_raw` arm copied from the features arm (`:938-942`):

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GradingConfig { pub enabled: bool, pub max_attempts: u32 }
impl Default for GradingConfig {
    fn default() -> Self { Self { enabled: false, max_attempts: 2 } } // matches max_review_fix_cycles
}
```

- **`grade_round` event payload** (new history event kind; recorded via `record_event`, `src/app/mod.rs:4164`):
  `{ "round": u32, "max_rounds": u32, "outcome": "working"|"pass"|"fail"|"skip", "command": String?,
  "exit_code": i64?, "critique": String? }`. Pure-of-state so `ChatProjection::rebuild` stays deterministic.
- **`grader_verdict` event payload**: the serialized `GraderVerdict` (durable record of the decisive check).
- **`ChatLifecycleKey::Grade { run_id }`** (new variant, `src/app/chat/mod.rs:115`) + `item_id()` arm
  (`chat:grade:{run_id}`); **`ChatItemKind::GradeLoop`** (new variant, `:25`) + `slug()` arm.
- **`PendingClarification`** (`src/app/mod.rs:366`) gains `grade_escalation: Option<GradeEscalation>` carrying
  `{ producing_agent_id, changed_files }` so the resume branch can identify a grade escalation and retry it.

### API Endpoints

Not applicable — this feature exposes no HTTP/RPC surface. Its "interfaces" are the `[grading]` TOML section,
the three escalation options in the TUI clarification picker, and the `grade_round`/`grader_verdict` events.

## Integration Points

Not applicable — no external services. All verification runs locally through the existing
`ActionRequest::RunCommand` path (`execute_run_command`, `src/actions/mod.rs:947`), inheriting the
`max_command_minutes` (default 10 min) timeout and the canonical-command auto-approval allowlist.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|-----------|-------------|---------------------|-----------------|
| `src/config/mod.rs` | new + modified | Add `GradingConfig`/`RawGradingConfig`, `EffectiveConfig.grading`, builtin default, `apply_raw` arm, init scaffold `[grading]` block. Low risk (additive, `deny_unknown_fields` requires the field + parser land together). | Implement; add config tests |
| `src/orchestrator/mod.rs` | new | Add `GraderVerdict`/`GradeOutcome` + `derive_grade_verdict`. Low risk (new types). | Implement + unit tests |
| `src/actions/mod.rs` | modified | Factor `is_canonical_verification_command` out of `is_default_read_only_command` (`:443`) for reuse. Low risk (refactor with shared set). | Extract shared predicate |
| `src/app/mod.rs` | new + modified | `run_grading_workflow`; trigger gate at `:3083`; verdict derivation from `command_completed`/`action_results`; `grade_escalation` on `PendingClarification`; tri-state branch in `resolve_pending_clarification` (`:1689`). **Highest-risk area** (touches the run loop + pause/resume). | Implement carefully; integration tests |
| `src/app/chat/mod.rs` | modified | New `ChatLifecycleKey::Grade` + `ChatItemKind::GradeLoop` (+ `item_id`/`slug` arms; non-exhaustive matches force updates). Low risk. | Implement |
| `src/app/chat/projection.rs` | new + modified | `grade_round` dispatch arm + `apply_grade_round` (mirror `apply_clarification_answered` `:1266`); keep within `MAX_BODY_LINES=12`. Medium risk (must not fall into the council `apply_diagnostic` non-collapsing path). | Implement + projection tests |
| Built-in `reviewer` instructions (`src/config/mod.rs:669`) | modified (minor) | Optionally sharpen to "run the project's tests/build/lint and report" so the agent-discovered grader reliably runs a canonical command. Low risk (prompt string). | Optional tuning |
| Runtime schema briefs (codex/claude/cursor) | unchanged | Verdict is harness-derived; no contract change. | None |

## Testing Approach

### Unit Tests

- **`derive_grade_verdict`** (the correctness core): table-driven over `CommandOutcome` lists — canonical
  command exit 0 → Pass; canonical non-zero → Fail (carries command + exit code); only non-canonical
  commands (`echo`, `ls`) → Skip; no commands → Skip; mixed canonical (one fails) → Fail.
- **`is_canonical_verification_command`**: matches `cargo test/check/build/clippy/fmt` (incl. with args),
  rejects `echo`/`cargo run`/chained `cargo test && ...` (shell-control already disqualifies upstream).
- **Config merge**: `[grading]` absent → `enabled=false, max_attempts=2`; values from home then local override;
  unknown `[grading]` key → hard parse error (`deny_unknown_fields`); `librarian`-style "present but off" test.
- **Escalation branch**: `resolve_pending_clarification` with `grade_escalation` set + `selected_option_id`
  `accept`/`retry`/`abort` routes to continue / re-grade / fail; a *normal* clarification with an option id
  `retry` is unaffected (marker-gated).
- **`apply_grade_round`**: successive `grade_round` events collapse into one `Grade` item; body accumulates
  round lines, stays ≤ `MAX_BODY_LINES`; status transitions working→fail→pass and working→…→fail(exhausted).

### Integration Tests

- **`FakeRuntime` end-to-end** (`tests/` + `src/runtime/fake.rs` control phrases): add phrases driving a
  grader to emit a canonical command with a controlled exit code and to drive `pass`/`fail`/`skip`. Assert:
  (1) an Edit step with changes triggers grading; (2) FAIL re-dispatches the *same* producing agent and the
  critique reaches it; (3) PASS concludes and the run continues; (4) grade/fix sub-steps do **not** advance
  `step_count` (run does not hit `max_agent_steps`); (5) exhaustion pauses with accept/retry/abort and each
  option behaves (continue / re-grade / fail); (6) SKIP when no canonical command runs; (7) `grading.enabled=false`
  (default) produces no grading at all; (8) chat shows one collapsing `Grade` item with the round counter.
- **Determinism note:** the verdict logic is fully covered by unit tests on synthetic command results; the
  integration tests assert *loop wiring, events, and projection*, controlling the grader's command/exit code
  through `FakeRuntime` rather than running a real toolchain.

## Development Sequencing

### Build Order

1. **Config `GradingConfig`** — no dependencies. `EffectiveConfig.grading`, `RawGradingConfig`, `apply_raw`,
   builtin default, init scaffold; config tests.
2. **Canonical-check predicate** — no dependencies. Factor `is_canonical_verification_command` out of
   `is_default_read_only_command`; unit tests.
3. **`GraderVerdict` + `derive_grade_verdict`** — depends on step 2 (uses the predicate). Pure types + deriver;
   exhaustive unit tests.
4. **`grade_round`/`grader_verdict` events + chat projection** — depends on step 3 (serializes verdict).
   New `ChatLifecycleKey::Grade`, `ChatItemKind::GradeLoop`, `apply_grade_round`; projection tests.
5. **`run_grading_workflow` executor** — depends on steps 1, 3, 4. The loop: dispatch grader via
   `execute_runtime_step_with_actions`, derive verdict, emit round event, re-dispatch producing agent on FAIL,
   bound by `grading.max_attempts`, no `step_count` increment.
6. **Trigger gate** — depends on step 5. Invoke `run_grading_workflow` from the `run_agent_step` `AgentResult`
   arm gated by `grading.enabled` + Edit capability + non-empty `changed_files` + top-level single-agent;
   map `Escalated`→`Paused`, `Concluded`→`Completed`.
7. **Escalation tri-state** — depends on steps 5, 6. `grade_escalation` marker on `PendingClarification`;
   on exhaustion build the accept/retry/abort pause; branch in `resolve_pending_clarification` before the
   prompt-append.
8. **FakeRuntime control phrases + end-to-end integration tests** — depends on steps 5–7.
9. *(Optional, out of MVP)* extract a shared `stop_for_agent_step_limit` helper — independent cleanup; no
   longer forced because grading is step-budget-exempt (see ADR-003).

### Technical Dependencies

- No external/infra dependencies. The only internal prerequisite is that step 6 (trigger) lands after the
  executor (step 5), and the executor after the verdict (step 3) and the config (step 1).

## Monitoring and Observability

- **Events (the metric substrate):** `grade_round` (`round`, `max_rounds`, `outcome`), `grader_verdict`
  (decisive command + exit code), reusing the durable `command_completed` (exit codes) and the existing
  `clarification_requested`/`clarification_answered` (escalation resolution).
- **PRD metrics derivable from events:** convergence = share of grade loops ending `pass` within
  `max_attempts`; false-pass = `pass` verdicts followed by a user revert/re-prompt; escalation actionability
  = accept vs retry/abort distribution; cost multiplier = grade/fix sub-step tokens vs run total.
- **Structured fields:** every grade event carries `run_id`/`step_id` for correlation; the `Grade` chat item
  carries the round counter for live visibility.
- **No alerting thresholds** (local single-user tool); the runaway guard is `max_attempts` + wall-clock +
  per-command timeout, surfaced as a visible counter rather than an alert.

## Technical Considerations

### Key Decisions

- **Harness-driven internal loop, re-dispatch the same agent** (ADR-003) — deterministic; reuses the
  `run_council_workflow` template and the action-executing step path. Trade-off: a new executor + a grader
  turn per attempt. Rejected: orchestrator-routed grading (discretionary fix, wrong agent).
- **Verdict derived from canonical-check exit codes** (ADR-004) — grounded, no config, no contract change.
  Trade-off: cargo-centric in V1 (non-cargo → SKIP until Phase 2). Rejected: trusting `AgentResult.status`
  (self-attested); any-command-all-zero (gameable).
- **Step-budget exemption** (ADR-003) — grade/fix sub-steps don't increment `run.step_count`, bounded by
  `max_attempts` + wall-clock + command timeout. Trade-off: a run can do more work than its step count
  suggests. Rejected: shared budget (starves the loop), separate `max_grading_steps` (redundant knob).
- **Reuse the clarification pause for escalation** (ADR-001) — the transport is already a multi-option
  picker; accept/retry/abort needs only a marker + a branch on the already-plumbed `selected_option_id`.

### Known Risks

- **Trigger over-firing** (subtasks, parallel children, grader/fixer re-entry) — *Likely without care.*
  Mitigation: gate to top-level single-agent Edit steps; sub-steps run off the `run_agent_step` arm so they
  cannot re-trigger. Parallel-edit grading is deferred (PRD open question).
- **Grade event invisible in chat** — *Medium.* The projection catch-all silently drops unknown kinds.
  Mitigation: the `grade_round` arm + projection test (step 4) before the executor depends on it.
- **Retry escalation loop** — *Medium.* Mitigation: "retry" resets the attempt budget via the
  `grade_escalation` context; it does not re-enter the exhausted state with the same counter.
- **Non-cargo projects silently unverified** — *Medium.* Mitigation: SKIP is surfaced honestly as
  "done-unverified" in chat; Phase 2's configured command makes verification mandatory.
- **Grader doesn't run a canonical command** — *Medium.* Mitigation: optionally sharpen the reviewer
  instruction; SKIP (never false PASS) when it doesn't. Needs validation during prototyping.

## Architecture Decision Records

- [ADR-001: Externally-grounded auto-verification loop, not an LLM self-grader](adrs/adr-001.md) — grounding
  mandatory; typed machine-derived verdict; skip-when-no-oracle; default OFF with a V2 flip criterion.
- [ADR-002: Phased delivery — agent-discovered verification in V1, config-asserted in Phase 2](adrs/adr-002.md)
  — MVP uses the agent-discovered command (verdict still exit-code-derived); authoritative config + discoverability follow.
- [ADR-003: Harness-driven bounded grade→fix loop, exempt from the run step budget](adrs/adr-003.md) — a
  `run_grading_workflow` executor re-dispatches the same agent, bounded by `max_attempts`, step-budget-exempt.
- [ADR-004: Harness-derived verdict from canonical-check exit codes](adrs/adr-004.md) — PASS requires a
  canonical `cargo` check at exit 0; else FAIL/SKIP; computed by the harness, not the agent.
