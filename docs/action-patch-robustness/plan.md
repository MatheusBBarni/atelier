# Action & Patch Robustness — Fix Plan

## Status

Engineering fix plan (diagnosis + proposal). Not yet implemented. Drafted
2026-06-13 from observed recurring action-failure errors in the run loop.

## Problem Statement

During multi-agent runs (notably the cy task-execution flow editing
`.compozy/tasks/<slug>/*.md`), agents intermittently emit invalid actions that
the harness denies or fails. The run then stalls and the user must manually type
`continue` to give the agent another attempt, after which it usually succeeds.

The harness is behaving *correctly* — it rejects malformed actions. The problem
is that (a) the model produces malformed actions too often, (b) the patch parser
is strict enough that small deviations are unrecoverable, and (c) repeated
failures exhaust the step's action budget and force manual intervention.

## Symptoms (error → cause → source)

| Observed message | Cause | Source |
|---|---|---|
| `Command: <unknown command>` denied | `run_command` action with no `command` param (display falls back to `<unknown command>`) | `src/actions/mod.rs:201,249` · `src/app/chat/projection.rs:737` |
| `rtk compozy tasks validate … exit 1` | A **real** non-zero command exit (validation failed because task files were mid-edit) — not a harness bug | the subprocess itself |
| `apply_patch action is missing diff` | `apply_patch` action with no `diff` param | `src/actions/mod.rs:183,237` |
| `no unified diff file patches found` | diff had bare `@@` hunks and **no `--- a/… / +++ b/…` file headers** | `src/actions/mod.rs:1264` |
| `invalid hunk marker "d"` | a context line missing its **leading space** (so `dependencies:` parses as marker `d`) | `src/actions/mod.rs:1236` |
| `patch context mismatch … expected "type: test", found "type: chore"` | the patch's context didn't match the **current** file (stale read / concurrent edit) | `src/actions/mod.rs:1371` |

## Root Causes

### 1. The model is never told the action `params` format

The runtime protocol prompt describes the action-request schema as `"params": {}`
— empty, with **zero** documentation. There is no spec or example of:

- the `apply_patch` `diff` field (unified diff with `--- a/… / +++ b/…` headers,
  `@@ -x,y +a,b @@` ranges, leading-space context lines), or
- the `run_command` `command` field.

Smaller models therefore guess the format and get it wrong.

Locations:

- `src/runtime/claude.rs:1017-1024`
- `src/runtime/cursor.rs:1049-1056`
- `src/runtime/codex.rs:336-343`

The structured-adapter system prompt (`src/runtime/claude.rs:31-38`) documents
the `orchestrator_decision` and `agent_result` output schemas in detail, but the
`action_request` `params` are left undocumented.

### 2. The unified-diff parser is strict and brittle

`parse_unified_diff` / `apply_hunks_to_text` (`src/actions/mod.rs:1172-1378`)
require:

- a `--- ` header followed by a `+++ ` header per file (`:1200`),
- a well-formed `@@ -x,y +a,b @@` hunk header (`parse_hunk_header:1285-1297`) —
  a bare `@@` is not even recognized as a hunk and yields
  `no unified diff file patches found`,
- exact body line counts matching the header (`:1243-1246`),
- a leading space on every context line (`:1223`; otherwise `invalid hunk marker`),
- exact line-number positioning with no offset/fuzzy search
  (`apply_hunks_to_text:1314`; `assert_source_line:1365-1378`).

There is no `patch(1)`-style fuzzy/offset matching, so any small deviation in
ranges, position, or whitespace is a hard reject.

### 3. Repeated failures exhaust the step budget → manual `continue`

There **is** an auto-retry loop: a denied/failed action result is pushed onto
`request.action_results` and the runtime is re-invoked so the agent can
self-correct (`src/app/mod.rs:3807-3933`). However, every failed attempt counts
against `max_step_actions` (`:3830-3840`). When the model keeps emitting bad
patches (root cause #1), it burns the step's action budget →
`stop_for_step_action_limit` → `StepOutcome::LimitReached`, the run halts, and
the user's `continue` starts a fresh step/budget where it finally succeeds.

There is no separate allowance for *recoverable malformed-action* retries versus
*successful work* actions.

## Fix Plan (prioritized)

### P1 — Document the action `params` format to the model (highest impact, lowest risk)

In the action-request schema section of `src/runtime/claude.rs`,
`src/runtime/cursor.rs`, and `src/runtime/codex.rs`, document each `kind`'s
`params` and add a concrete valid example. This eliminates most malformed
patches at the source.

```
apply_patch.params.diff (unified diff):
--- a/path
+++ b/path
@@ -1,3 +1,3 @@
 context line (leading space)
-removed line
+added line
 context line
• context lines start with a space; removals "-"; additions "+"
• include the @@ -start,count +start,count @@ range
• for full rewrites or new files, use write_file instead

run_command.params.command: the exact shell command string
```

### P2 — Sharper corrective feedback on denial/failure

When `src/actions/mod.rs` denies or fails a patch, return a *specific* fix hint
inside the `ActionResult` rather than a terse error, e.g.:

- "context line 4 needs a leading space",
- "context mismatch — re-read the file; current line 4 is `type: chore`",
- "missing `diff`; expected unified-diff format: …".

This lets the agent self-correct *within* the step's budget instead of
exhausting it.

### P3 — Make patch application tolerant

In `parse_unified_diff` (`:1172-1267`) and `apply_hunks_to_text` (`:1308-1341`):

- derive line counts from the hunk body instead of trusting the header (drop the
  `:1243` mismatch bail),
- apply hunks by **searching for the context** near the stated line (offset /
  fuzzy matching, `patch(1)`-style) rather than requiring exact line numbers —
  this turns most "context mismatch" and bad-range cases into successful
  applies.

The missing-leading-space and missing-file-header cases are ambiguous to repair
safely and are better addressed by P1.

### P4 — Don't require manual `continue`

Either:

- (a) a bounded **auto-resume** when a step hits `LimitReached` purely from
  recoverable malformed-action churn (no real progress), or
- (b) a separate, larger retry allowance for *failed* actions versus *successful*
  ones, so transient format mistakes do not consume the work budget.

Target: `src/app/mod.rs:3830-3840` (`stop_for_step_action_limit`) and the
`LimitReached` → run-state handling.

## Recommended Sequence

**P1 → P2 → P4 → P3.**

P1 and P2 are small prompt/feedback changes that should eliminate most of the
`continue` interruptions; P4 removes the manual babysitting; P3 is the larger
parser-hardening effort.

## Operational Note (not a code fix)

The `type: test` vs `type: chore` context mismatch is the signature of
**concurrent runs editing the same `.compozy/tasks/<slug>/*` files**. Running one
agent per file-set (rather than overlapping runs with auto-commit) avoids that
entire class of "context mismatch" failures.
