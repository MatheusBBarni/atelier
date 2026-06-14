# Technical Specification: Action & Patch Robustness

**Document Status:** Draft
**Version:** 1.0
**Author:** MatheusBBarni
**Date:** 2026-06-13
**Last Updated:** 2026-06-13
**Source documents:** `docs/action-patch-robustness/plan.md` (diagnosis),
`docs/action-patch-robustness/prd.md` (requirements)

## Table of Contents

- [Executive Summary](#executive-summary)
- [1. Background](#1-background)
- [2. Goals & Non-Goals](#2-goals--non-goals)
- [3. Architecture Overview](#3-architecture-overview)
- [4. Module A — Patch Engine](#4-module-a--patch-engine-deep-module-extracted)
- [5. Module B — Action Diagnostics / Fix-Hint mapping](#5-module-b--action-diagnostics--fix-hint-mapping)
- [6. Module C — Runtime Action-Protocol brief](#6-module-c--runtime-action-protocol-brief)
- [7. Module D — Budget separation + bounded auto-resume](#7-module-d--budget-separation--bounded-auto-resume)
- [8. Data Model & Config Changes](#8-data-model--config-changes)
- [9. Testing Strategy](#9-testing-strategy)
- [10. Migration & Backward Compatibility](#10-migration--backward-compatibility)
- [11. Risks & Mitigations](#11-risks--mitigations)
- [12. Implementation Phases](#12-implementation-phases)
- [13. Acceptance Criteria](#13-acceptance-criteria)
- [14. Open Questions](#14-open-questions)

---

## Executive Summary

**Problem:** Multi-agent runs (most visibly the cy task-execution flow editing
`.compozy/tasks/<slug>/*.md`) stall when an agent emits a malformed action — an
`apply_patch` with no `diff`, a diff with no file headers, a context line missing
its leading space, or a stale context. The harness correctly rejects these, but
the terse errors plus a single shared per-step action budget force the user to
type `continue` so a fresh budget can finish the work.

**Solution:** Four coordinated changes — (A) extract the unified-diff
parser/applier into a deep, tolerant **patch engine** with a typed error;
(B) map every rejection cause to a **specific corrective hint** fed back to the
model; (C) document each action `kind`'s `params` in all three **runtime briefs**;
(D) **separate** recoverable malformed-action retries from the real-work budget
and **bounded-auto-resume** churn-only stalls.

**Impact:** Runs that stalled two or three times finish unattended; failures that
remain become self-describing; no change to approvals, capabilities, or workspace
write policy.

---

## 1. Background

### 1.1 Current run/action flow

`App::execute_runtime_step_with_actions` (`src/app/mod.rs:3802-3939`) is the
per-step loop:

1. `drive_runtime_step_streaming` invokes the runtime and yields a
   `RuntimeOutput` (`src/runtime/mod.rs:380`). For an `ActionRequest`, the loop
   continues; any other output ends the step.
2. Before executing, it checks
   `limit_reached(&max_step_actions, request.action_results.len())`
   (`:3835-3838`). `Limit::is_reached_by` is `value >= limit`
   (`src/config/mod.rs:93-98`); `max_step_actions` defaults to **20**
   (`Limit::default_step_actions`, `src/config/mod.rs:85-87`).
3. The action is validated (`validate_action_request_with_scope`,
   `src/actions/mod.rs:152-260`) and executed (`execute_action_request`,
   `:274-350`).
4. **Every** `ActionResult` — success, denial, or failure — is pushed onto
   `request.action_results` (`:3934`) and sent back to the runtime via the
   prompt envelope (`prompt_envelope_json`, `src/runtime/mod.rs:766-798`,
   serializes `action_results`).
5. When the combined count reaches the limit, `stop_for_step_action_limit`
   (`src/app/mod.rs:3552-3569`) sets `RunState::LimitReached` and the loop
   returns `StepOutcome::LimitReached`. The user's `continue` starts a fresh
   step with a fresh budget.

So a denied/failed action already triggers an automatic re-invocation (the agent
*can* self-correct) — but each failed attempt consumes the same budget as real
work, and the error it gets back is terse.

### 1.2 The patch path

- `execute_apply_patch` (`src/actions/mod.rs:957-974`) → `apply_unified_diff`
  (`:987-1025`): scans for binary markers, calls `parse_unified_diff`
  (`:1172-1267`), resolves+reads each target, calls `apply_hunks_to_text`
  (`:1308-1341`), and **stages all rewrites in a map, writing only after every
  hunk on every file applies** (`:1019-1022`). This atomicity is asserted by
  `execute_apply_patch_is_atomic_for_context_mismatch` (`:1806-1846`) and **must
  be preserved**.
- The same `parse_unified_diff` is re-used by `validate_unified_diff_for_policy`
  (`:1027-1035`) and `validate_parallel_patch_scope` (`:705-714`), so it is
  already the single structural gate for patches.

Strictness sources (all hard `bail!`s):
- missing `--- `/`+++ ` headers → `no unified diff file patches found` (`:1263`)
- bare `@@` not recognized as a hunk (same symptom)
- header-vs-body count mismatch (`:1243-1247`)
- context line missing leading space → `invalid hunk marker` (`:1219-1236`)
- exact-position context check (`assert_source_line`, `:1365-1378`)
- rename / `/dev/null` create-delete (`:1189-1196`), binary (`:992-994`)

### 1.3 Protocol-brief gap

`claude_prompt_text` (`src/runtime/claude.rs:958-1039`), `cursor_prompt_text`
(`src/runtime/cursor.rs:990-…`), and `codex_prompt_text`
(`src/runtime/codex.rs:278-…`) each render the action contract with
`"params": {{}}` and zero field docs (`claude.rs:1017-1024`,
`cursor.rs:1049-1056`, `codex.rs:336-343`), while `orchestrator_decision` and
`agent_result` are fully specified. The structured-adapter system prompt
(`claude.rs:38`) already keeps small models in contract mode; only the params
documentation is missing.

### 1.4 Constraint discovered during grounding (load-bearing)

A **real command non-zero exit** returns `ActionStatus::Failed` with diagnostic
`"command returned a non-zero exit status"` (`execute_run_command`,
`src/actions/mod.rs:901-923`) — the **same status** a malformed patch produces.
Therefore the budget classifier (Module D) **cannot** treat `status == Failed` as
"recoverable churn"; doing so would auto-resume a genuinely failing
`compozy tasks validate` forever. Recoverability must be an **explicit signal**
set only at the format-error construction sites. This drives the `ActionResult`
field added in §8.

No ADRs exist for the actions/runtime/patch area (`grep` of `.compozy` and
`docs` returned none), so there are no binding decisions to honor beyond the code.

---

## 2. Goals & Non-Goals

### Goals

1. Eliminate most malformed actions at the source (documented params).
2. Make every rejection self-correctable with a specific hint.
3. Apply patches tolerant of header counts and small positional offsets, without
   ever applying to a location the engine isn't confident about.
4. Stop charging recoverable format mistakes against the work budget, and remove
   the manual `continue` for churn-only stalls.
5. Extract the patch logic into a deep module testable on pure string fixtures.

### Non-Goals

- New action kinds or action-schema changes beyond documenting `params` and the
  additive `recoverable_failure` flag.
- Relaxing approvals, capabilities, workspace roots, or command policy.
- `apply_patch` file creation/deletion/rename (write_file remains the path for
  new files) or binary patches.
- Three-way/merge conflict resolution. Tolerance is bounded offset/context
  search only — **never fuzzy content equality**.
- Fuzzy-matching *removed* or *context* line **content** (these must match
  exactly; only the *position* is searched).

---

## 3. Architecture Overview

```
                         ┌────────────────────────────────────────────┐
   runtime brief (C) ───▶│ model emits action_request {kind, params}   │
                         └───────────────┬────────────────────────────┘
                                         ▼
        src/app/mod.rs  execute_runtime_step_with_actions  (loop, D)
          ├─ classify prior results: work vs recoverable_failure
          ├─ dual budget: work→max_step_actions, churn→max_recoverable_failures
          ├─ bounded auto-resume on churn-only ceiling (max_auto_resumes)
          ▼
        src/actions/mod.rs  execute_action_request
          ├─ validate (policy/scope) ── ActionDecision::Denied{reason, recoverable}
          ├─ execute_apply_patch ─────▶ patch::apply (A) ─▶ PatchError (A)
          └─ build ActionResult { status, diagnostic ←fix_hint (B), recoverable }
          ▼
        src/actions/patch.rs  (NEW deep module, A+part of B)
          parse(diff) -> Result<Vec<FilePatch>, PatchError>
          apply(original, &FilePatch) -> Result<String, PatchError>
          PatchError::fix_hint() -> String        (B)
```

Module boundaries map 1:1 to the PRD modules. A (patch engine) is the only new
file; B lives partly in the engine (`fix_hint`) and partly at the action
construction sites; C is a shared constant in `src/runtime/mod.rs`; D is the loop
+ config.

---

## 4. Module A — Patch Engine (deep module, extracted)

### 4.1 Location & surface

New file **`src/actions/patch.rs`**, declared `mod patch;` from
`src/actions/mod.rs`. Public-in-crate surface (the stable interface):

```rust
pub(crate) struct FilePatch {
    pub target_path: String,
    pub hunks: Vec<Hunk>,        // Hunk/HunkLine stay private to the module
}

/// Parse a unified diff into per-file patches. Structural validation only;
/// does not touch the filesystem.
pub(crate) fn parse(diff: &str) -> Result<Vec<FilePatch>, PatchError>;

/// Apply one file's hunks to its current text, tolerant of header counts and
/// small positional offsets. Pure: text in, text out.
pub(crate) fn apply(original: &str, patch: &FilePatch) -> Result<String, PatchError>;
```

`Hunk`, `HunkLine`, `parse_hunk_header`, `parse_hunk_range`, `parse_diff_path`,
`normalize_diff_path`, `split_lines_preserving_endings`, and the context-equality
check move into the module (private). `actions/mod.rs` keeps the **I/O
orchestration** (`apply_unified_diff`: resolve paths, read, atomic stage+write,
duplicate-target guard) and calls `patch::parse` + `patch::apply`.
`validate_unified_diff_for_policy` and `validate_parallel_patch_scope` call
`patch::parse`. Binary/rename/`/dev/null` checks move **into** `patch::parse` as
`PatchError` variants (today they are split between `apply_unified_diff` and
`parse_unified_diff`), giving one structural source of truth.

### 4.2 Typed error

```rust
pub(crate) enum PatchError {
    MissingFileHeaders,                                  // no --- / +++ pair found
    MissingHunkHeader,                                   // bare @@ / unparseable range
    InvalidContextMarker { line: usize, found: char },   // e.g. 'd' from "dependencies:"
    ContextMismatch { line: usize, expected: String, found: String },
    ContextNotFound { near_line: usize, expected: String },
    HunkOutOfBounds { start: usize },
    DuplicateTarget(String),
    Unsupported(&'static str),                           // binary / rename / create-delete
}
```

Implements `std::error::Error` + `Display`. `apply_unified_diff` maps
`PatchError` into the existing `anyhow` flow via `?`/`.map_err`, so callers that
expect `anyhow::Result` are unaffected. The `fix_hint()` method (§5) is the
actionable text.

### 4.3 Tolerance algorithm (`apply`)

The header `@@ -old_start,old_count +new_start,new_count @@` becomes **advisory**.
Per hunk:

1. Build the hunk's expected old-side block = the ordered `Context` + `Remove`
   lines (the lines that must exist in the source, in order).
2. Search `source_lines` for that exact block, starting at `old_start-1` and
   expanding outward by increasing offset up to a bounded window
   `PATCH_SEARCH_WINDOW` (proposed **64** lines each direction; also clamp to file
   bounds). "Exact" = trailing-newline-trimmed string equality, identical to
   today's `assert_source_line` (`:1369`).
3. **First match wins**, preferring the smallest absolute offset (search `0,
   +1, -1, +2, -2, …`) so the closest plausible position is chosen.
4. If found at position `p`: emit `source_lines[cursor..p]` verbatim, then walk
   the hunk applying `Context`→copy, `Remove`→skip, `Add`→insert
   (`format!("{content}\n")`, as today `:1332-1334`); advance the cursor.
5. If no position in the window matches the full block:
   - if the leading context line is found somewhere but a later line diverges →
     `ContextMismatch { line, expected, found }` (carries the diverging source
     line so B can show "line N is now X").
   - else → `ContextNotFound { near_line, expected }`.
6. Counts are **derived from the body** (count `Context`/`Remove`/`Add`); the
   header counts are not compared (drop the `:1243` bail). Hunks still apply
   left-to-right with a monotonic cursor, so overlapping/out-of-order hunks that
   would move the cursor backward → `HunkOutOfBounds`.

**Why this is safe:** only *position* is searched; context and removed lines
still require exact content equality, so a stale patch whose removed line no
longer exists (the atomic test's `-two` vs current `TWO`) finds no valid position
→ `Err` → no write. Atomicity (§1.2) is unchanged because `apply` is pure and
`apply_unified_diff` still stages all files and writes only on full success.

### 4.4 What stays a hard reject (handled by C/B, not by guessing)

`MissingFileHeaders`, `MissingHunkHeader`, `InvalidContextMarker` (missing
leading space), `Unsupported` (binary/rename/create-delete), `DuplicateTarget`.
These are ambiguous to repair automatically; C documents the format to prevent
them and B tells the agent exactly what to fix.

---

## 5. Module B — Action Diagnostics / Fix-Hint mapping

### 5.1 `PatchError::fix_hint()`

A pure method returning the corrective string for each variant:

| Variant | `fix_hint()` |
|---|---|
| `MissingFileHeaders` | "diff needs `--- a/<path>` then `+++ b/<path>` headers before the `@@` hunk." |
| `MissingHunkHeader` | "include an `@@ -start,count +start,count @@` range before the hunk body." |
| `InvalidContextMarker{line,found}` | "context line {line} needs a leading space (found `{found}` as the marker)." |
| `ContextMismatch{line,expected,found}` | "context mismatch — re-read the file; line {line} is now `{found}`, not `{expected}`. Rebase the patch on current contents." |
| `ContextNotFound{near_line,..}` | "couldn't locate the patch context near line {near_line} — re-read the file and rebase the hunk." |
| `HunkOutOfBounds{start}` | "hunk at line {start} overlaps or precedes a prior hunk; order hunks top-to-bottom." |
| `DuplicateTarget(p)` | "patch lists `{p}` twice; one hunk set per file." |
| `Unsupported(r)` | "{r}; for new files or full rewrites use write_file." |

### 5.2 Non-patch (param) denials

Set at their construction sites in `actions/mod.rs`:

- missing `diff` (`:182-184`, `:236-238`) → "apply_patch needs a `diff` in unified
  format: `--- a/path` / `+++ b/path`, an `@@ -s,c +s,c @@` range, context lines
  starting with a space, removals `-`, additions `+`. For new files use
  write_file."
- missing `command` (`:200-202`, `:248-250`, `:308-316`) → "run_command needs a
  `command` string (the exact shell command to run)."

### 5.3 Plumbing

The corrective string is placed in `ActionResult.diagnostic` (already wired):
`execute_apply_patch` matches `PatchError`, builds `Failed` with
`diagnostic = err.fix_hint()`; param denials build `Denied` with the hint. The
diagnostic flows to the model via `action_results` in the envelope (the primary
P2 channel — guaranteed every retry) and to chat wherever the projection already
renders diagnostics (`record_action_specific_events`, `src/app/mod.rs:4597-4624`;
file-edit/command result items). No projection change is required for the model
feedback; chat simply shows richer text.

---

## 6. Module C — Runtime Action-Protocol brief

### 6.1 Shared fragment

Add to `src/runtime/mod.rs` (next to `prompt_envelope_json`):

```rust
pub(crate) const ACTION_PROTOCOL_BRIEF: &str = r#"
"params" by kind:
  read_file    { "path": "relative/path" }
  list_files   { "path": "relative/dir" }
  search_text  { "path": "relative/dir", "query": "text" }
  run_command  { "command": "the exact shell command string" }
  write_file   { "path": "relative/path", "content": "full file contents" }
  apply_patch  { "diff": "<unified diff, see below>" }
  record_note  { "note": "text" }

apply_patch.diff (unified diff):
--- a/path
+++ b/path
@@ -1,3 +1,3 @@
 context line (leading space)
-removed line
+added line
 context line
- context lines start with a space; removals "-"; additions "+"
- include the @@ -start,count +start,count @@ range
- for new files or full rewrites, use write_file instead
"#;
```

### 6.2 Wiring into the three builders

Each `*_prompt_text` builder interpolates the fragment as a named `format!` arg
(e.g. `action_protocol = crate::runtime::ACTION_PROTOCOL_BRIEF`) immediately
after the existing `"params": {{}}` action block in `claude.rs:1024`,
`cursor.rs:1056`, `codex.rs:343`. The `{{ }}` JSON braces in those `format!`
templates are unaffected because the fragment is injected via a placeholder, not
inlined. No behavioral change to the adapter framing (still "no tools; one
contract per turn").

---

## 7. Module D — Budget separation + bounded auto-resume

### 7.1 Recoverability signal

Per §1.4, an explicit signal is required. Two contained changes in
`src/actions/mod.rs` (neither type is serialized except `ActionResult`, handled
in §8):

```rust
// internal enum, not serialized — safe to extend
enum ActionDecision { Allowed, RequiresApproval(String), Denied { reason: String, recoverable: bool } }
```

`ActionResult` gains `recoverable_failure: bool` (§8). It is set **true** only
for the format-error set:
- missing `diff` / missing `command` denials,
- `execute_apply_patch` errors whose `PatchError` is a format/context variant
  (all variants in §4.2 **except** `Unsupported(binary)` — binary is treated as
  non-recoverable since the agent shouldn't retry binary patches).

It stays **false** for: completed actions, **real command non-zero exits**
(§1.4), policy/capability/scope denials, path traversal, VCS denials, write_file
overwrite refusals, and missing-file errors. (write_file overwrite already
redirects to apply_patch via its message; classify conservatively as
non-recoverable to avoid resume loops.)

### 7.2 Dual budget in the loop

Replace the single check at `src/app/mod.rs:3835-3845` with classification over
the accumulated results (counts derived each iteration — no new request field):

```rust
let work = request.action_results.iter().filter(|r| !r.recoverable_failure).count() as u32;
let churn = request.action_results.len() as u32 - work;

if limit_reached(&self.config.limits.max_step_actions, work) {
    self.stop_for_step_action_limit(run, &step_id, request.action_results.len())?;
    return Ok(StepOutcome::LimitReached);            // real-work budget: unchanged behavior
}
if limit_reached(&self.config.limits.max_recoverable_failures, churn) {
    if auto_resumes_used < self.config.limits.max_auto_resumes.value_or(0) {
        auto_resumes_used += 1;
        self.record_auto_resume(run, &step_id, auto_resumes_used)?;   // event for chat/telemetry
        // append a sharper corrective nudge as a synthetic note in action_results,
        // then fall through and keep looping (effective churn ceiling rises one increment)
    } else {
        self.stop_for_step_action_limit(run, &step_id, request.action_results.len())?;
        return Ok(StepOutcome::LimitReached);        // churn exhausted after bounded resumes
    }
}
```

`auto_resumes_used` is a loop-local `u32`. The "nudge" is an appended
`record_note`-style entry summarizing the repeated failure class so the next
attempt gets stronger guidance. Net behavior: real work halts at
`max_step_actions` exactly as today; format churn gets its own
`max_recoverable_failures` allowance, extended in up to `max_auto_resumes`
bounded increments before finally stopping — so a transiently-confused agent
recovers unattended while a permanently-broken one still stops.

### 7.3 Guardrails

- A real command failure (`Failed`, `recoverable_failure == false`) counts as
  work and is never auto-resumed.
- Policy/capability/scope denials count as work (non-recoverable) → a genuinely
  disallowed action still stops the step.
- Auto-resume emits an event so the chat transcript and `--doctor`/telemetry show
  it happened (not silent).

---

## 8. Data Model & Config Changes

### 8.1 `ActionResult` (additive, history-compatible)

`src/actions/mod.rs:53-62`:

```rust
pub struct ActionResult {
    pub schema_version: u32,
    pub action_id: String,
    pub status: ActionStatus,
    pub summary: String,
    pub content: Option<Value>,
    pub artifact: Option<Value>,
    pub diagnostic: Option<String>,
    #[serde(default)]                       // old persisted events deserialize as false
    pub recoverable_failure: bool,          // NEW
}
```

`#[serde(default)]` keeps existing `.atelier/` history events readable (they
deserialize with `recoverable_failure == false`). `schema_version` stays `1`
(additive, default-valued field — no breaking change to consumers). `action_result(..)`
helper (`:774-790`) keeps its signature and sets `recoverable_failure: false`;
the format-error sites set it explicitly.

### 8.2 `Limits` config

`src/config/mod.rs` — add two limits alongside `max_step_actions`, following the
existing `Limit` enum + `#[serde(default)]` + merge pattern (`:174-186`,
`:445`, `:850-851`):

| Field | Type | Default | Meaning |
|---|---|---|---|
| `max_recoverable_failures` | `Limit` | `Value(10)` | Per-step allowance for recoverable format failures, separate from work. |
| `max_auto_resumes` | `Limit` | `Value(2)` | Bounded auto-resume increments before a churn-only step halts. |

Both overridable in `[limits]` TOML and via the CLI-flag merge path. Document in
the sample/`--print-config` output (`:2121` area).

---

## 9. Testing Strategy

A good test asserts **external behavior through the public interface** — feed an
input, assert the observable output — never private helpers or struct internals.
Per the PRD's chosen scope, dedicated new suites cover **Module A** and
**Module B** only; C and D ride existing infrastructure.

### 9.1 Patch Engine (Module A) — `src/actions/patch.rs` `#[cfg(test)]`

Pure-function tests on string fixtures (prior art: the literal-fixture tests in
`src/actions/mod.rs:1380+`, e.g. `execute_apply_patch_is_atomic_for_context_mismatch`).

- `parse` happy path → expected `FilePatch` count/targets.
- `apply` happy path → expected text (port the existing atomic test's success
  case: `-two/+TWO`).
- **count derived from body**: header `@@ -1,2 +1,1 @@` with a 1-line body
  applies (today this `bail!`s — see §10).
- **offset tolerance**: a hunk whose context sits a few lines from `old_start`
  applies at the found position; beyond `PATCH_SEARCH_WINDOW` → `ContextNotFound`.
- **exact-content safety**: stale removed line (`-two` vs current `TWO`) →
  `ContextMismatch`/`ContextNotFound`, **no** false apply (the correctness
  guarantee).
- hard rejects each map to their variant: missing headers → `MissingFileHeaders`;
  bare `@@` → `MissingHunkHeader`; missing leading space → `InvalidContextMarker`;
  binary/rename/`/dev/null` → `Unsupported`; duplicate target → `DuplicateTarget`.

### 9.2 Fix-Hint mapping (Module B)

Table test: each `PatchError` variant and each param denial → its specific
`fix_hint()` string, including that `ContextMismatch` surfaces the current line
and `InvalidContextMarker` names the line + leading-space fix. Prior art: the
diagnostic-string assertions in `apply_patch_policy_rejects_*`
(`src/actions/mod.rs:1849-1897`).

### 9.3 Not given dedicated suites (per PRD)

- **Module D** — verified via the existing `FakeRuntime` end-to-end app tests
  (`src/runtime/fake.rs` control phrases; `src/app/mod.rs` tests like
  `limit_reached_run_does_not_replay_queued_items:9622`). Add a control phrase
  that emits recoverable malformed patches and assert the run **completes** (not
  `LimitReached`) within the resume bound, and that a phrase emitting a real
  failing command still halts.
- **Module C** — extend an existing prompt test (e.g.
  `cursor_prompt_text_uses_harness_actions:1674`) with a light assertion that the
  rendered brief contains the documented params and the `apply_patch` example. No
  new suite.

### 9.4 Gate

`cargo fmt --check && cargo clippy --all-targets && cargo test --locked` (CI
mirror). Scope edits to the touched files; do not run a blanket `cargo fmt` over
the tree (parallel WIP convention).

---

## 10. Migration & Backward Compatibility

- **Behavior change to an existing test:**
  `apply_patch_policy_rejects_rename_and_hunk_count_mismatch`
  (`src/actions/mod.rs:1868-1897`) asserts the count-mismatch patch
  `@@ -1,2 +1,1 @@\n-old\n+new\n` is **denied** with `"hunk line count mismatch"`.
  After §4.3 (counts derived from body) this no longer denies at parse/policy
  time. **Action:** split the test — keep the rename-rejection assertion; move
  the count case into the engine tests asserting it now **parses** (and applies
  when context matches). This is an intended robustness change, called out so the
  green→change is deliberate, not a silent regression.
- **History compatibility:** `ActionResult.recoverable_failure` is
  `#[serde(default)]`; previously-persisted `.atelier/` events load unchanged.
- **Config compatibility:** new limits default to sensible values; existing
  `atelier.toml` files need no edits.
- **Callers of `apply_unified_diff` / `parse_unified_diff`:** the public
  `apply_unified_diff` signature is unchanged; internal callers switch to
  `patch::parse`/`patch::apply`. No external API surface changes.
- **Atomicity invariant preserved** (§4.3) — the atomic test stays green.

---

## 11. Risks & Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Offset search applies a hunk to the wrong (but matching) location | Silent wrong edit | Require exact content equality for context+remove lines; smallest-offset-first; bounded window; atomic stage+write; correctness test in §9.1. |
| Auto-resume masks a real, repeating failure | Wasted tokens / hidden bug | Real failures are `recoverable_failure=false` (never resumed, §1.4/§7.3); resume is bounded by `max_auto_resumes` and emits an event. |
| Misclassifying a recoverable failure as work (or vice-versa) | Wrong budget accounting | Flag set centrally at the few format-error sites; conservative default false; FakeRuntime test for both directions (§9.3). |
| `ActionResult` field breaks history decode | Corrupt session load | `#[serde(default)]`; explicit load test against a pre-change fixture event. |
| Tolerance regresses a previously-rejected malformed patch into a bad apply | Data loss in edited file | Hard rejects retained for ambiguous cases (§4.4); exact-content rule; atomic write. |
| Larger/duplicated brief inflates prompt tokens | Cost/latency on small models | Single shared constant (~15 lines) replaces the empty block; net token delta is small and offsets retries. |

---

## 12. Implementation Phases

Sequence follows the PRD: **C → B → D → A-tolerance**, with the A **extraction**
landing first as the structural prerequisite for B's typed errors.

- **Phase 0 — Extract patch engine (A, no behavior change).** Create
  `src/actions/patch.rs`; move parse/apply/types; route policy+scope+execute
  through it; introduce `PatchError` mapping `1:1` to today's `bail!`s. All
  existing tests green. *Refactor only.*
- **Phase 1 — C (documented params).** Shared `ACTION_PROTOCOL_BRIEF`; wire into
  three builders; light prompt-test assertion.
- **Phase 2 — B (fix hints).** `PatchError::fix_hint()` + param-denial hints;
  populate `ActionResult.diagnostic`; fix-hint tests.
- **Phase 3 — D (budget + auto-resume).** `recoverable_failure` field +
  `ActionDecision::Denied{recoverable}`; classify; dual budget; bounded resume;
  config limits; FakeRuntime test.
- **Phase 4 — A tolerance.** Body-derived counts + offset search; update the
  migrated test (§10); engine tolerance + correctness tests.

Each phase is independently shippable and leaves the gate green.

---

## 13. Acceptance Criteria

1. A cy task run that previously required one or more manual `continue`s due to
   malformed patches completes unattended (FakeRuntime regression).
2. A malformed `apply_patch` (missing space, bad count, small offset) is either
   applied (offset/count) or returned with a specific `fix_hint` the agent uses
   to self-correct within the step.
3. A stale/conflicting patch never applies to the wrong location and leaves the
   target file unchanged (atomicity + exact-content tests green).
4. A real failing command (`compozy tasks validate … exit 1`) still halts the
   step and is **not** auto-resumed.
5. All three runtime briefs render the documented `params` and a valid
   `apply_patch` example.
6. `cargo fmt --check && cargo clippy --all-targets && cargo test --locked`
   passes; the deliberately-changed test (§10) is updated, not deleted.
7. Existing `.atelier/` history sessions load without error after the
   `ActionResult` field addition.

## 14. Open Questions

1. `PATCH_SEARCH_WINDOW` value — proposed 64 lines each direction. Tune against
   real `.compozy/tasks` edit sizes; could be a `[limits]` knob if needed.
2. `max_recoverable_failures` / `max_auto_resumes` defaults (10 / 2) — confirm
   against observed churn before merge.
3. Should the auto-resume "nudge" be a visible chat item or telemetry-only? Spec
   assumes a recorded event surfaced minimally in chat; confirm desired
   verbosity.
4. Whether write_file overwrite-refusal should be recoverable (currently
   classified non-recoverable, conservative). Low stakes; revisit if it shows up
   as a churn source.
