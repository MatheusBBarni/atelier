# TechSpec: In-Session Security Review

## Executive Summary

`/security-review` adds a manual, advisory, branch-diff security audit to atelier. The command is dispatched from the async `submit_prompt_with_source` path into a new `run_security_review_workflow` (a sibling of `run_council_workflow`) that: resolves the branch diff in app code, redacts it, embeds it as untrusted data in a read-only reviewer agent's prompt, dispatches one runtime step, parses the returned `AgentResult.findings` into shared `Finding`/`Severity` types, and records a `security_review_started`/`security_review_completed` event family. A new `apply_security_review` projection arm collapses those events into one evolving `SecurityReview` chat card (scope line → verdict header → severity-grouped findings → disclaimer), reusing the grade-loop card pattern.

The **primary technical trade-off** (ADR-004): findings cross the runtime boundary as formatted strings in the existing `AgentResult` envelope and are parsed app-side, rather than via a new typed runtime schema. This keeps the change runtime-agnostic and additive (no runtime adapter touches the contract) at the cost of a tolerant string parser with a safe fallback. A second deliberate trade-off (ADR-005): the review runs as a self-contained async flow under its own `review_id`, never touching `RunState`, so it cannot destabilize the orchestrator — at the cost of a dedicated lifecycle key and default-branch/merge-base resolution with several edge-case fallbacks.

## System Architecture

### Component Overview

- **Command surface** (`src/slash_commands.rs`): one `AppCommand` catalog entry `/security-review`. Dispatch branch in `submit_prompt_with_source` (`src/app/mod.rs`) with an active-run guard.
- **Review workflow** (`src/app/mod.rs`, new `run_security_review_workflow`): orchestrates gather → redact → dispatch → parse → record. Owns the `review_id`. Does not read/mutate `RunState`.
- **Diff acquisition** (`src/app/git.rs`, extended): default-branch + merge-base resolution and a `git diff <base>` fetch following the existing `tokio::process::Command` + `kill_on_drop` + timeout pattern; plus `redact_diff`.
- **Reviewer agent** (`src/config/mod.rs`, new built-in): `security-reviewer` profile, `capabilities = [Read]` (no `Command`/`Edit`), instructions = inline rubric constant (confidence gate, hard-exclusion list, line-format contract, untrusted-content framing).
- **Shared domain types** (`src/orchestrator/mod.rs`, new): `Severity`, `Finding`, and `parse_finding_line` — colocated with `GraderVerdict`, reusable by the V2 cross-runtime gate.
- **Projection** (`src/app/chat/mod.rs` + `projection.rs`, extended): `ChatItemKind::SecurityReview`, `ChatLifecycleKey::SecurityReview { review_id }`, and `apply_security_review` modeled on `apply_grade_round`.

**Data flow:** user types `/security-review` → guard checks no active run → workflow mints `review_id`, records `security_review_started` (Scanning card) → resolves base + fetches + redacts diff → builds `RuntimeRequest` (diff embedded in prompt, `capability_constraints = [Read]`) → `execute_runtime_step_streaming` → `AgentResult` → `parse_finding_line` per entry + curation → `security_review_completed { findings, scope, model, truncated }` → `apply_security_review` renders the report card.

## Implementation Design

### Core Interfaces

Shared leaf types (in `src/orchestrator/mod.rs`, beside `GraderVerdict`):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Critical, High, Medium, Low, Info }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub location: Option<String>, // "src/db.rs:42"
    pub why: Option<String>,      // why it is exploitable
    pub fix: Option<String>,      // advisory remediation, never applied
}

/// Tolerant parser: "[HIGH] title — loc — why — fix".
/// Never fails — unrecognized input becomes a Medium finding with raw title.
pub fn parse_finding_line(line: &str) -> Finding { /* … */ }
```

Workflow + diff entry points:

```rust
// src/app/mod.rs (impl App)
async fn run_security_review_workflow(&mut self) -> Result<()>;

// src/app/git.rs
/// Resolves default branch (origin/HEAD → main → master), computes merge-base
/// with HEAD, returns the diff vs working tree. None when there are no changes.
pub async fn fetch_branch_diff(dir: &Path) -> Result<Option<BranchDiff>>;

pub struct BranchDiff { pub base_label: String, pub text: String, pub files: usize }

pub fn redact_diff(diff: &str) -> String; // masks credential-shaped lines
```

Built-in reviewer profile (constructed in `src/config/mod.rs`): `capabilities = vec![Capability::Read]`, `tools = None`-but-read-only, `instructions = SECURITY_REVIEWER_RUBRIC`, `model` = a capable default with `model_fallbacks`.

### Data Models

- **`Finding` / `Severity`** — above; the shared leaf types.
- **`security_review_started` payload**: `{ "review_id", "scope": { "base_label", "files", "truncated": bool }, "model" }` → renders the Scanning card + scope line.
- **`security_review_completed` payload**: `{ "review_id", "model", "scope": {…}, "findings": [Finding…], "summary" }` → renders the verdict header, severity-grouped findings, disclaimer.
- **Chat card** (`ChatItemView` with `kind = SecurityReview`): `status` Pending→Completed; `severity` = max finding severity mapped to `ChatSeverity` (Critical/High→Error/Warning, none→Info — never `Success`-as-"secure"); `body` = scope line + grouped `[SEV] …` lines (reusing `ChatLineStyle::Warning/Error/Muted`) + persistent disclaimer line.

### Command Surface (no HTTP API)

This is a TUI command, not a network API. Surface = one catalog entry:

| Command | Args | Behavior |
|---------|------|----------|
| `/security-review` | none (V1) | Runs a review of the current branch diff; declines if a run is active. |

## Integration Points

All boundaries are in-process; no external services. Internal integrations: the `git` binary (subprocess, short timeout, `kill_on_drop`), the runtime layer (`execute_runtime_step_streaming`, runtime-agnostic), and the event/projection pipeline (`record_event` → `apply_history_event` → `sync_chat_items`). Reuses redaction predicates from `src/file_index.rs` (`is_secret_name`/`is_secret_dir`).

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|-----------|-------------|----------------------|-----------------|
| `src/orchestrator/mod.rs` | modified | Add `Severity`, `Finding`, `parse_finding_line`. Low risk (additive types). | Define types + tolerant parser with fallback. |
| `src/app/git.rs` | modified | Add branch-diff base resolution + `redact_diff`. Medium risk (git edge cases). | Implement fallbacks + per-case unit tests. |
| `src/config/mod.rs` | modified | New built-in `security-reviewer` agent + rubric const. Low risk (additive default). | Add profile via `insert_builtin_agent`; pick default model. |
| `src/app/mod.rs` | modified | New `run_security_review_workflow` + dispatch branch + active-run guard. Medium risk (async flow, run-id handling). | Mirror `run_council_workflow`; mint `review_id`; guard. |
| `src/app/chat/mod.rs` | modified | New `ChatItemKind::SecurityReview`, `ChatLifecycleKey::SecurityReview`. Low risk (additive enum variants — check exhaustive matches). | Add variants; fix match arms. |
| `src/app/chat/projection.rs` | modified | New `apply_security_review` + dispatch arm. Medium risk (projection correctness). | Model on `apply_grade_round`; test card evolution. |
| `src/slash_commands.rs` | modified | New catalog entry (ADR-frozen catalog exception per ADR-001). Low risk. | Add `SlashCommandSpec`; keep help/dropdown aligned. |
| `src/runtime/fake.rs` | modified | Control-phrase branch emitting deterministic findings. Low risk (test-only). | Add fake security-review result. |
| `src/tui/mod.rs` | modified | Render the `SecurityReview` card kind. Low risk (reuses severity tokens; respect `colors_live_only_in_theme_module`). | Map kind to existing theme tokens. |

## Testing Approach

### Unit Tests
- **`parse_finding_line`**: well-formed lines for every severity; missing fields; reordered/garbage lines → safe fallback (never panics, never drops). Adversarial lines (injection text inside a finding).
- **Diff base resolution** (`fetch_branch_diff`): normal branch vs `main`; detached HEAD; no default branch; first commit; no upstream; empty diff. Each asserts the resolved `base_label` and fallback path.
- **`redact_diff`**: credential-shaped lines (`.env`, `*.pem`, `api_key=`, tokens) masked; ordinary code preserved; reuses `file_index` predicates.
- **Severity → `ChatSeverity` mapping**: never maps "no findings" to a green "secure" affordance.

### Integration Tests (full run through `FakeRuntime`, in `src/app/mod.rs` `#[tokio::test]`)
- Control phrase `security findings: critical` → `security_review_started` then `security_review_completed` events present; `findings` array structured; card status `Completed`, severity `Error`.
- Clean diff → "no changes / no high-confidence findings surfaced" card with scope line + disclaimer, never "secure".
- Active-run guard: invoking `/security-review` mid-run declines without dispatching.
- Truncation: oversized fake diff → `scope.truncated = true` reflected in the card.
- **Prompt-injection corpus** (ADR-002): fake diffs carrying suppression / capability-escalation / secret-bait text assert (a) seeded finding still reported, (b) reviewer issues no out-of-scope action (it has only `Read`), (c) redaction masks secret-bait before it reaches the event/transcript.

## Development Sequencing

### Build Order
1. **Shared types + parser** (`Severity`, `Finding`, `parse_finding_line` in `orchestrator/mod.rs`) — no dependencies.
2. **Diff + redaction** (`fetch_branch_diff`, base resolution, `redact_diff` in `app/git.rs`) — no dependencies (parallelizable with step 1).
3. **Built-in reviewer agent + rubric const** (`config/mod.rs`) — no dependencies (parallelizable with 1–2).
4. **Projection** (`ChatItemKind::SecurityReview`, `ChatLifecycleKey::SecurityReview`, `apply_security_review` + dispatch arm) — depends on **step 1** (Finding payload shape).
5. **Review workflow** (`run_security_review_workflow`: gather → redact → dispatch → parse → curate → record) — depends on **steps 1, 2, 3**.
6. **Command wiring** (catalog entry + `submit_prompt_with_source` branch + active-run guard) — depends on **step 5**.
7. **Test infrastructure** (FakeRuntime control phrase + unit/integration/injection-corpus tests) — depends on **steps 1–6**.
8. **TUI rendering polish** (card kind → theme tokens, `/help` line) — depends on **steps 4, 6**.

### Technical Dependencies
None external. A capable default reviewer model must be chosen (Open Question in PRD); until then the profile points at an existing default with a fallback chain.

## Monitoring and Observability

- **Events** (the audit trail): `security_review_started` / `security_review_completed`, each carrying `review_id`, `model`, `scope`, and (on completion) `findings`. These drive the Success Metrics: adoption (count of `started`), latency (`started`→`completed` timestamps), findings-per-review and severity mix (`completed.findings`), recall (CI eval set), truncation rate (`scope.truncated`).
- **Structured fields**: `review_id`, `base_label`, `files`, `truncated`, `finding_count`, `max_severity`, `model`.
- No alerting (local tool); the events are the observability surface and feed the eval harness.

## Technical Considerations

### Key Decisions
- **Decision:** Reuse `AgentResult` + parse findings app-side (ADR-004). **Rationale:** runtime-agnostic, additive, no adapter changes. **Trade-off:** tolerant string parsing vs typed end-to-end. **Rejected:** new runtime schema (touches every adapter); strings-only (loses shared types).
- **Decision:** Self-contained async flow with its own `review_id`, never touching `RunState` (ADR-005). **Rationale:** advisory action must not destabilize a run. **Trade-off:** new lifecycle key + base-resolution edge cases. **Rejected:** blocking-like-council (freezes input); attach-to-current-run (concept/lifecycle muddle).
- **Decision:** Reviewer `capabilities = [Read]`, diff injected as data (ADR-001/002). **Rationale:** structural read-only + removes general `Command` from the prompt-injection blast radius. **Trade-off:** reviewer can't run tools to validate a finding (can read files only). **Rejected:** agent runs `git diff` itself (re-ships the Anthropic CVE class).
- **Decision:** Rubric as built-in inline instructions, not a discovered skill (ADR-004). **Rationale:** built-in reliability regardless of workspace skill files. **Trade-off:** less user-editable. **Rejected:** skill-file dependency.

### Known Risks
- **Finding-line parser fragility** (medium likelihood). *Mitigation:* tolerant parser with safe fallback (never drops a finding); unit tests over malformed/adversarial lines; the rubric enforces the format.
- **Base resolution misreporting scope** (medium). *Mitigation:* deterministic fallback order; resolved base always printed in the card; per-edge-case tests.
- **Prompt injection via the diff** (medium, high impact). *Mitigation:* read-only reviewer (no `Command`/`Edit`), untrusted-content framing, redaction before transcript, CI injection corpus.
- **Additive enum variants break exhaustive matches** (low, compiler-caught). *Mitigation:* compiler surfaces all match sites; address during steps 4–6.
- **Default model too weak → confident noise** (medium). *Mitigation:* sane default + F6 weak-model warning surfaced in the card.

## Architecture Decision Records

- [ADR-001: Standalone read-only reviewer + app-orchestrated diff-as-data workflow, advisory and diff-scoped, own event family](adrs/adr-001.md) — V1 mechanism and data model.
- [ADR-002: The security reviewer is a hostile-input boundary — diff-as-data, read-only, redacted findings, honest disclaimer, CI injection corpus](adrs/adr-002.md) — threat model and guardrails.
- [ADR-003: Security review output is a read-only, scope-honest "security report" card](adrs/adr-003.md) — output product shape (Approach B).
- [ADR-004: Findings cross the runtime boundary as AgentResult lines parsed app-side into shared Severity/Finding types; rubric as built-in instructions](adrs/adr-004.md) — findings data path.
- [ADR-005: Self-contained async review flow with its own id — merge-base-vs-default-branch diff, truncate-with-note, never touching RunState](adrs/adr-005.md) — execution and scoping model.
