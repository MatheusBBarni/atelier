# TechSpec: Cross-Runtime Verification Gate — `/review`

## Executive Summary

`/review` is a new `AppCommand` that runs a single **opinion-only** reviewer over the current uncommitted `git diff`, selecting a reviewer whose **model family** (derived from `RuntimeKind → ProviderId`) is absent from the **session-level producer-family set**, and surfacing structured `ReviewFinding`s as one coalescing `ReviewRound` chat item. The app acquires the diff itself (new `git.rs` helpers) and embeds it in the reviewer prompt, so the reviewer needs only `Read`/`Review` capability — no action gate, no approval prompt, strictly advisory.

The implementation maximizes reuse: the command handler copies the `handle_provider_status_command` shape; reviewer dispatch reuses `execute_runtime_step`; the reviewer profile copies `council_member_agent`; and the chat item copies the `GradeLoop` lifecycle pattern (`grade_key`/`apply_grade_round`). **Primary trade-off:** family is resolved at provider granularity (no model-string parsing) and the producer set is session-level rather than diff-attributed — a deliberately conservative superset that is simpler and never under-excludes, at the cost of occasional false-SKIPs and inability to distinguish two families behind one runtime (neither exists today).

## System Architecture

### Component Overview

- **`/review` command (entry).** Catalog entry in `slash_commands.rs`; dispatch guard + `handle_review_command` in `src/app/mod.rs`. Orchestrates the flow and records events. Returns `Ok(true)`.
- **Diff acquisition.** New helpers in `src/app/git.rs` (`working_diff`, `changed_files`) reusing the `run_git` primitive. Returns the unified diff + file list, or `None` (no repo / no changes).
- **Family resolver.** New helper in `src/runtime/status.rs`: agent → configured runtime → `RuntimeKind` → `ProviderId`. Single source of "family."
- **Producer-set tracker.** Session-accumulated set of families of Edit-capable steps, sourced from provenance fields newly added to the `agent_step_started` payload; reconstructable from history on resume.
- **Reviewer selector + engine.** Auto-selects an enabled agent whose family ∉ producer set, builds an opinion-only `AgentProfile`, dispatches one `execute_runtime_step`, parses `ReviewFinding`s, records review events — or records a `review_skipped` event.
- **Review chat item.** `ReviewFinding` data type + `ReviewRound` lifecycle item (`ChatItemKind::ReviewRound`, `ChatLifecycleKey::Review`, `apply_review_round`) coalescing `review_started`/`review_finding`/`review_completed`.
- **Feedback.** 👍/👎 on a finding emits `review_finding_rated` (no state mutation).

**Data flow:** `/review` → `working_diff()` → compute producer-family set → select reviewer (or SKIP) → `execute_runtime_step` with diff-in-prompt → parse `ReviewFinding`s → record `review_started`/`review_finding*`/`review_completed` → `apply_review_round` coalesces into one chat item.

## Implementation Design

### Core Interfaces

The primary new domain type other components depend on:

```rust
// src/review/mod.rs (new module) — structured finding, machine-readable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity { Important, Nit }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingConfidence { High, Medium, Low }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewFinding {
    pub severity: FindingSeverity,
    pub file: String,
    pub line: Option<u32>,        // verify-before-surface: dropped if no file:line
    pub claim: String,            // one-line scannable claim
    pub rationale: String,        // progressive-disclosure body
    pub confidence: FindingConfidence,
}
```

Family resolver and command handler signatures:

```rust
// src/runtime/status.rs — the single "family" source (no model-string parsing).
pub fn agent_family(config: &EffectiveConfig, agent: &AgentProfile) -> Option<ProviderId>;

// src/app/mod.rs — guard handler, modeled on handle_provider_status_command.
async fn handle_review_command(&mut self, prompt: &str) -> Result<bool>;

// src/app/git.rs — new diff helpers reusing run_git.
pub async fn working_diff(dir: &Path) -> Option<WorkingDiff>; // { unified: String, files: Vec<String> }
```

Reviewer selection outcome:

```rust
// src/review/mod.rs
pub enum ReviewerSelection {
    Selected { agent_id: String, reviewer_family: ProviderId },
    Skipped { producer_families: Vec<ProviderId>, reason: String },
}
```

### Data Models

- **`ReviewFinding`** (above) — carried in `review_finding` event payloads, not `AgentResult.findings`.
- **`ReviewRound` chat item** — `ChatItemKind::ReviewRound`; `ChatLifecycleKey::Review { run_id }` with an `item_id()` arm (`"chat:review:{run_id}"`). Header line: reviewer family + producer-family set + tally.
- **Provenance fields** — `agent_step_started` payload extended from `{agent}` to `{agent, runtime, family}` (family = `ProviderId` snake_case). Back-compat: history events lacking `family` contribute `unknown` (permissive).
- **No new config.** Reviewer is auto-selected; `[review]` override is explicitly deferred (ADR-005).

### Command & Event Surface

| Surface | Form | Description |
|---|---|---|
| `/review` | AppCommand (catalog `AppCommand` kind) | Request an independent review of the working diff |
| `review_started` | event | `{run_id, reviewer_agent, reviewer_family, producer_families, file_count}` → opens the `ReviewRound` item |
| `review_finding` | event | one `ReviewFinding` → appends a line to the item |
| `review_completed` | event | `{counts_by_severity}` → terminal status on the item |
| `review_skipped` | event | `{producer_families, reason}` → visible SKIP diagnostic |
| `review_finding_rated` | event | `{finding_ref, rating}` → precision-metric signal |

## Integration Points

All integration is in-crate; no external services. The feature reuses existing internal boundaries:

- `run_git` (`src/app/git.rs`) — extended with `git diff` / `git diff --name-only`.
- `execute_runtime_step` (`src/runtime/mod.rs:466`) — single-step reviewer dispatch.
- `record_event` (`src/app/mod.rs`) + the `upsert` projection engine (`src/app/chat/projection.rs`) — event recording and chat coalescing.
- `ProviderId` (`src/runtime/status.rs`) — family taxonomy.
- Runtime structured-output schema briefs (`codex`/`claude`/`cursor`) — extended to express `ReviewFinding`.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/slash_commands.rs` | modified | New `/review` catalog entry; 3 enumeration tests hard-code the V1 label set and will fail. Low risk, mechanical. | Add entry + update `FIXED_V1_LABELS` and the two catalog tests |
| `src/app/mod.rs` (dispatch + handler) | modified | New guard before `reject_unknown_slash_command` + `handle_review_command`. Medium risk (core run path). | Add handler modeled on `handle_provider_status_command` |
| `src/app/git.rs` | modified | New `working_diff`/`changed_files` via `run_git`. Low risk. | Add helpers + timeout/truncation handling |
| `src/runtime/status.rs` | modified | New `agent_family` resolver. Low risk. | Add helper using `ProviderId::from(kind)` |
| `agent_step_started` payload | modified | Add `runtime`/`family`. Risk: consumers/tests asserting exact payload. | Extend payload at all emit sites; update assertions |
| `src/review/mod.rs` | new | `ReviewFinding`, selection, engine. Adding a single file (YAGNI: no new crate). | Create module |
| `src/app/chat/mod.rs` + `projection.rs` | modified | `ReviewRound` kind + `Review` key + `apply_review_round` + dispatch arms. Medium risk (variant-drift tests). | Mirror `GradeLoop` plumbing |
| Runtime schema briefs (`codex`/`claude`/`cursor`) | modified | Reviewer structured-output must express `ReviewFinding`. Low/medium. | Extend the review output schema |
| `src/runtime/fake.rs` | modified | New control phrase to script findings + family. Low risk (test-only). | Add branch in `fake_agent_result`/`stream_step` |

## Testing Approach

### Unit Tests
- **`agent_family`**: each `RuntimeKind` → expected `ProviderId`; unknown/unmapped → `None`.
- **Producer-set computation**: given recorded step events with families, the session set is the correct union; missing-family steps → `unknown` (permissive, not excluded).
- **Reviewer selection**: picks an agent whose family ∉ producer set; returns `Skipped` with the producer list when every enabled family is in the set; deterministic ordering when multiple are eligible.
- **`ReviewFinding` parsing/validation**: well-formed structured output parses; findings without `file:line` are dropped (verify-before-surface).
- **`apply_review_round` projection**: `review_started` opens one item; multiple `review_finding`s append into the same item (one `ChatLifecycleKey::Review`); `review_completed` sets terminal status; `review_skipped` renders a clear SKIP.
- **Diff helpers**: `working_diff` returns `None` for non-repo / clean tree; truncates oversized diffs with a marker.

### Integration Tests
- **End-to-end via `fake` runtime** (`tests/`/`src/app` test module): build an `App` with a producer agent on one `RuntimeKind` and an enabled agent on a different kind; run a producing step, then `app.submit_prompt("/review")`; assert: (1) `review_started.reviewer_family` ≠ any `producer_families`; (2) `review_finding` events recorded (scripted by a new fake control phrase); (3) the chat projection yields exactly one `ReviewRound` item with the findings.
- **SKIP path**: single-family config → `/review` records `review_skipped` naming the producer family, and emits no reviewer step.
- **No approval prompt**: assert the opinion-only reviewer step runs in `normal` mode without a pending approval.
- **Resume**: a session resumed from history reconstructs the producer-family set from the persisted `family` fields.

## Development Sequencing

### Build Order
1. **Family resolver** (`agent_family` in `src/runtime/status.rs`) — no dependencies.
2. **Diff helpers** (`working_diff`/`changed_files` in `src/app/git.rs`) — no dependencies.
3. **`ReviewFinding` type + module** (`src/review/mod.rs`) + reviewer structured-output brief — no dependencies.
4. **Provenance fields + producer-set tracker** — extend `agent_step_started` payload and accumulate the session set; **depends on 1**.
5. **Reviewer selector + engine** (auto-select, opinion-only profile, single `execute_runtime_step`, record review events / `review_skipped`) — **depends on 1, 2, 3, 4**.
6. **`/review` command wiring** (catalog entry + tests, dispatch guard, `handle_review_command`) — **depends on 5**.
7. **`ReviewRound` chat lifecycle** (`ChatItemKind`/`ChatLifecycleKey`/`review_key`/`apply_review_round`/dispatch arms) — **depends on 3 and the events from 5–6**.
8. **Feedback** (`review_finding_rated` + 👍/👎 key handling) — **depends on 7**.
9. **Fake-runtime control phrase + tests** (deterministic findings/family) — **depends on 5, 6, 7**.

### Technical Dependencies
- None external. All work is in-crate; reuses `run_git`, `execute_runtime_step`, `record_event`, and the `upsert` projection engine. Requires ≥2 runtimes of different `RuntimeKind` configured for the feature to do anything but SKIP (a user-config prerequisite, surfaced by the SKIP message).

## Monitoring and Observability

- **Events** (structured): `review_started` (`reviewer_family`, `producer_families`, `file_count`), `review_finding` (severity, confidence, `file:line`), `review_completed` (counts by severity), `review_skipped` (`producer_families`, `reason`), `review_finding_rated` (rating).
- **Metrics derivable from events:** family-diversity correctness (`reviewer_family ∉ producer_families` — must be 100% of non-skipped), SKIP rate, findings-per-review and distribution by confidence, dismiss rate (from ratings), tokens per `/review` (cost overhead).
- **Logs:** reviewer selection decision (chosen agent + family, or skip reason); diff size and truncation.

## Technical Considerations

### Key Decisions
- **App-acquired diff + opinion-only reviewer** (ADR-004) — avoids any approval surface and keeps diff acquisition deterministic; trade-off: large diffs cost prompt tokens (mitigated by truncation), and the reviewer can't run tests (by design — the deterministic gate owns that).
- **Provider-level family via `RuntimeKind`** (ADR-005) — reuses the only existing signal; trade-off: can't split two families on one runtime (none exist today); localized so a model-string refinement can replace it later.
- **Session-level producer set** (ADR-005) — conservative superset; trade-off: occasional false-SKIP, surfaced clearly; precise diff-attribution deferred (ADR-003 records the exact-set intent).
- **Structured `ReviewFinding` + `ReviewRound` item** (ADR-006) — machine-readable for gating/metrics and legible rendering; trade-off: more plumbing than reusing `Vec<String>`/`GradeLoop`.

### Known Risks
- **Oversized working diffs** exceed the prompt budget → cap embedded diff size, mark truncation in the header, prefer changed-file list when over the cap.
- **Provider-level family coarseness** → acceptable today; isolated helper for future refinement.
- **Resumed/legacy history lacks the `family` field** → treat missing as `unknown` (permissive); never fabricate a family.
- **Reviewer returns malformed findings** → validate structured output; drop unlocated findings; cap nit volume.
- **Variant-drift tests** enumerate `ChatItemKind`/`ChatLifecycleKey`/catalog labels → update in the same change.
- **Default config has no diverse council reviewer** (all HttpApi) → auto-selection draws from *all* enabled agents (e.g. Codex agents), not just council members, so diversity is found when it exists.

## Architecture Decision Records

- [ADR-001: Lineage-based reviewer-diversity policy over council, advisory, provenance-grounded](adrs/adr-001.md) — record lineage as fact; decide on family; advisory; floor vs ceiling.
- [ADR-002: V1 ships as on-request `/review` — single independent reviewer over the working diff, advisory](adrs/adr-002.md) — Approach A; finding anatomy; panel deferred.
- [ADR-003: Independence over the producer-family *set*, loud SKIP on collapse](adrs/adr-003.md) — reviewer family ∉ set; `unknown` permissive.
- [ADR-004: `/review` runs an opinion-only reviewer over an app-acquired git diff, reusing single-step dispatch](adrs/adr-004.md) — app runs `git diff`; no approval surface.
- [ADR-005: Family from `RuntimeKind→ProviderId`; session-level producer set; auto-selected reviewer; provenance on the step event](adrs/adr-005.md) — no model-string parsing; conservative superset.
- [ADR-006: Structured `ReviewFinding` carried in review events, rendered as one coalescing `ReviewRound` chat item](adrs/adr-006.md) — machine-readable findings; `GradeLoop`-style lifecycle.
