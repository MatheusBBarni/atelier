# Action & Patch Robustness — PRD

## Status

Ready for agent. Synthesized 2026-06-13 from `docs/action-patch-robustness/plan.md`
(engineering diagnosis) plus a grounding pass over the action, runtime, and
run-loop code. Scope: all four prioritized fixes (P1–P4). Saved as a local PRD
(not published to an issue tracker per the invoking instruction).

## Problem Statement

When I run a multi-agent task in atelier — most visibly the cy task-execution
flow that edits `.compozy/tasks/<slug>/*.md` — the run keeps stalling on me. An
agent emits an action the harness can't accept, the run halts with a terse error,
and I have to type `continue` by hand to give the agent another attempt. After I
do, it usually succeeds on the next try. So the work is achievable, but I'm
babysitting the run and the failures are noisy and unexplained:

- `apply_patch action is missing diff`
- `no unified diff file patches found`
- `invalid hunk marker "d"`
- `patch context mismatch … expected "type: test", found "type: chore"`
- `Command: <unknown command>` denied

From my seat these read like the harness being fragile, even though it is
correctly rejecting malformed actions. The real causes are that the model is
never told the action `params` format, the unified-diff parser rejects any small
deviation outright, and every failed attempt eats the same per-step action budget
as real work — so a few format mistakes exhaust the step and force the manual
`continue`.

## Solution

Make malformed actions self-correcting and stop the run from halting on
recoverable format churn. Four changes, from the user's perspective:

1. **Tell the model the exact action format up front.** Each runtime's protocol
   brief documents every action `kind`'s `params` and shows one valid
   `apply_patch` example, so agents stop guessing the unified-diff shape.

2. **When an action is rejected, say exactly how to fix it.** Instead of a terse
   error, the harness returns a specific corrective hint ("context line 4 needs a
   leading space"; "context mismatch — re-read the file; current line 4 is
   `type: chore`"). The agent repairs and retries within the same step.

3. **Apply patches the way `patch(1)` does — tolerantly.** Line counts are
   derived from the hunk body, and hunks are located by searching for their
   context near the stated line rather than demanding an exact position. Most
   "context mismatch" and bad-range cases just apply.

4. **Stop making me type `continue`.** Recoverable malformed-action retries no
   longer consume the work budget, and a step that stalls purely on format churn
   auto-resumes within bounds instead of halting for manual intervention.

The net effect: runs that used to stall two or three times now finish unattended,
and when something genuinely can't be applied the error tells the agent (and me)
what to do about it.

## User Stories

1. As an atelier operator running a cy task, I want runs to finish without me
   typing `continue`, so that I can leave a multi-step task running unattended.
2. As an operator, I want a malformed `apply_patch` to be repaired by the agent
   automatically, so that one bad diff doesn't stall the whole run.
3. As an operator, I want the harness's rejection messages to tell me what was
   wrong and how to fix it, so that when a run does stop I understand why at a
   glance.
4. As an operator, I want format mistakes to not count against the real work
   budget, so that an agent doing legitimate work isn't cut off by a few early
   guesses at the diff format.
5. As an operator, I want a step that's churning on format errors with no real
   progress to recover on its own within a bounded number of attempts, so that I
   am not the recovery mechanism.
6. As an operator, I want the auto-resume to be bounded, so that a genuinely
   stuck agent still stops instead of looping forever and burning tokens.
7. As an agent (model) executing a step, I want the protocol brief to document
   each action `kind`'s `params`, so that I emit a correct contract on the first
   attempt instead of guessing.
8. As an agent, I want a concrete valid `apply_patch` example in the brief, so
   that I reproduce the exact unified-diff shape (file headers, `@@` ranges,
   leading-space context lines) the harness expects.
9. As an agent, I want to be told to use `write_file` for new files or full
   rewrites, so that I don't emit `/dev/null` create/delete diffs the harness
   rejects.
10. As an agent that emitted a diff with a missing leading space, I want the
    failure to name the offending line and the fix, so that my next attempt
    corrects exactly that line.
11. As an agent whose patch context no longer matches the file, I want the
    failure to tell me the file changed and show the current line, so that I
    re-read and rebase my patch instead of resubmitting the stale one.
12. As an agent that omitted the `diff` param, I want the rejection to restate
    the required unified-diff format with an example, so that I can fill it in
    without another round-trip.
13. As an agent that omitted the `command` param on `run_command`, I want the
    rejection to say a `command` string is required, so that I resubmit a
    well-formed command.
14. As an agent submitting a patch whose hunk header count is slightly off, I
    want the patch to still apply when the body is unambiguous, so that an
    off-by-one in the `@@` range doesn't fail valid work.
15. As an agent submitting a patch whose hunk is a few lines from the stated
    position (because the file shifted), I want the harness to find the context
    nearby and apply it, so that small offsets don't fail.
16. As an operator using the Claude runtime, I want the documented action params
    in its protocol brief, so that smaller Claude models stay in contract mode
    and stop producing malformed patches.
17. As an operator using the Cursor runtime, I want the same documented action
    params, so that behavior is consistent across runtimes.
18. As an operator using the Codex runtime, I want the same documented action
    params, so that behavior is consistent across runtimes.
19. As an operator, I want the patch logic isolated in its own module with a
    small interface, so that its behavior is well-tested and predictable
    independent of the action-execution and policy code.
20. As a maintainer, I want the unified-diff parser/applier extracted into a deep
    module I can test on pure string fixtures, so that I can add tolerance rules
    with confidence and no TUI/runtime setup.
21. As a maintainer, I want patch failures expressed as a typed error rather than
    free-form strings, so that the corrective-hint mapping is exhaustive and
    can't silently drift.
22. As a maintainer, I want each failure class mapped to exactly one corrective
    message, so that I can unit-test that mapping in isolation.
23. As an operator, I want tolerance to stop at genuinely ambiguous cases
    (missing file headers, missing leading space) rather than guess, so that the
    harness never applies a patch to the wrong place.
24. As an operator, I want a real, non-recoverable failure (a true command
    non-zero exit, a denied-by-policy action) to still stop the run, so that
    auto-resume never masks an actual problem.
25. As an operator, I want concurrent edits to the same file to be reported as a
    context mismatch with a re-read hint, so that the agent rebases rather than
    clobbering another run's change.
26. As an operator, I want the chat transcript to show the corrective hint on a
    failed action, so that I can follow the agent's self-correction in the UI.
27. As an operator, I want a `run_command` with no command to still surface a
    clear reason rather than the bare `<unknown command>` label, so that the
    failure is legible in chat.
28. As a maintainer, I want the patch tolerance behaviors documented as decisions
    (count derivation, offset search, what stays a hard reject), so that future
    changes don't silently loosen safety.
29. As an operator on a `normal`-approval workspace, I want the new feedback and
    tolerance to leave the approval gate untouched, so that write/command actions
    still prompt me exactly as before.
30. As a maintainer, I want the patch engine's interface to stay stable
    (`parse` + `apply`) even as tolerance rules evolve, so that callers and the
    diagnostics mapping don't churn.

## Implementation Decisions

### Module A — Patch Engine (deep module, extracted)

The unified-diff parser/applier and its helpers move out of the action-execution
file into a dedicated **patch engine** module with a small, stable interface and
no dependency on action policy, capabilities, runtimes, or the TUI. This is the
deep module the work is organized around; tolerance and typed errors live here.

Interface (the decision — names indicative, not paths):

- `parse(diff: &str) -> Result<Vec<FilePatch>, PatchError>`
- `apply(original: &str, patch: &FilePatch) -> Result<String, PatchError>`

Behavioral decisions:

- **Derive line counts from the hunk body**, not the `@@` header. Drop the
  current header-vs-body count-mismatch bail; a correct body with a slightly
  wrong header count applies.
- **Locate hunks by context search near the stated line** (`patch(1)`-style
  offset/fuzzy matching) instead of requiring exact line numbers. A hunk whose
  context is found within a bounded window of `old_start` applies at the found
  position; this turns most "context mismatch at line N" and bad-range cases into
  successful applies.
- **Keep hard rejects for the genuinely ambiguous / unsafe cases**: missing
  `--- a/… / +++ b/…` file headers, a bare `@@` with no recognizable range, a
  context line missing its leading space, binary patches, rename patches, and
  `/dev/null` create/delete diffs. These cannot be repaired safely by guessing
  and are addressed at the source by Module C (documented params) and Module B
  (a corrective hint that points the agent at the fix). Tolerance must never
  apply a patch to a location the engine isn't confident about.

Failures are a typed error so the diagnostics mapping (Module B) is exhaustive.
The variant shape encodes the decision (from the diagnosis of observed errors):

```rust
enum PatchError {
    MissingFileHeaders,                                 // "no unified diff file patches found"
    MissingHunkHeader,                                  // bare @@ / no range recognized
    InvalidContextMarker { line: usize, found: char },  // e.g. "d" from "dependencies:"
    ContextMismatch { line: usize, expected: String, found: String },
    ContextNotFound { near_line: usize, expected: String }, // search window exhausted
    HunkOutOfBounds { start: usize },
    Unsupported(&'static str),                           // binary / rename / create-delete
}
```

The existing policy/scope checks that today re-parse the diff
(`validate_unified_diff_for_policy`, parallel-scope validation) call into the same
engine, so there is one source of truth for "what is a valid patch."

### Module B — Action Diagnostics / Fix-Hint mapping

A pure mapping from a rejection cause to a specific, actionable `diagnostic`
string carried back on the `ActionResult`, so the agent self-corrects within the
step's budget rather than receiving a terse error. It covers both the policy
denials (missing `diff`, missing `command`) and every `PatchError` variant.

The corrective catalog (the decision):

- **missing `diff`** → "apply_patch needs a `diff` in unified format: `--- a/path`
  / `+++ b/path`, an `@@ -s,c +s,c @@` range, context lines starting with a space,
  removals `-`, additions `+`. For new files or full rewrites use write_file."
- **missing `command`** → "run_command needs a `command` string (the exact shell
  command to run)."
- **`InvalidContextMarker { line, found }`** → "context line {line} needs a
  leading space (found marker `{found}`)."
- **`ContextMismatch { line, expected, found }`** → "context mismatch — re-read
  the file; line {line} is now `{found}`, not `{expected}`. Rebase the patch on
  the current contents."
- **`ContextNotFound`** → "couldn't locate the patch context near line N —
  re-read the file and rebase the hunk."
- **`MissingFileHeaders` / `MissingHunkHeader`** → restate the required header /
  range shape with the example.
- **`Unsupported(reason)`** → restate the unsupported case and point at
  write_file where applicable.

This text is what flows back in `action_results` to the runtime, and it is what
the chat projection renders for a failed/denied action.

### Module C — Runtime Action-Protocol brief (shared, documented params)

The three runtime protocol briefs (Claude, Cursor, Codex) currently render the
action contract with an empty `"params": {}` and no field documentation, while
the `orchestrator_decision` and `agent_result` schemas are fully specified. The
decision is to document each `kind`'s `params` and include one valid `apply_patch`
example, rendered identically across all three runtimes (a single shared brief
fragment rather than three drifting copies).

The documented block (the decision):

```
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
• context lines start with a space; removals "-"; additions "+"
• include the @@ -start,count +start,count @@ range
• for new files or full rewrites, use write_file instead
```

This stays inside the existing structured-adapter framing (no tools; one contract
per turn) — it only fills in the previously-empty params section.

### Module D — Action Budget separation + bounded auto-resume

Today the run loop pushes every `ActionResult` (success or failure) into
`action_results` and checks the combined length against `max_step_actions`
(default 20, reached at `>=`); when the model churns on bad patches it exhausts
the step → `LimitReached`, the run halts, and the user's `continue` opens a fresh
budget where it finally succeeds. The decision is two complementary changes:

- **Separate allowance (primary, root-cause).** Classify each result:
  *recoverable malformed-action failure* (a format rejection the agent can repair
  — the Module B cases) vs *real work* (a completed action, a policy denial, or a
  true command non-zero exit). Recoverable failures draw on their own allowance
  and do **not** consume the `max_step_actions` work budget. The classifier is a
  small pure predicate over `ActionResult`.
- **Bounded auto-resume (safety net).** If a step still reaches `LimitReached`
  with no real progress (all churn), automatically resume for a bounded number of
  attempts instead of requiring a manual `continue`. A step that made real
  progress, or a non-recoverable failure, halts as it does today.

Guardrails: a true command non-zero exit (e.g. `compozy tasks validate … exit 1`),
a policy denial, and an approval-required action are **not** recoverable churn and
must not be auto-resumed or exempted from the work budget. Auto-resume is capped
so a genuinely stuck agent still stops.

### Cross-cutting decisions

- The approval gate, capability checks, workspace read/write roots, and
  `ApprovalMode` semantics are unchanged. None of these changes loosen what the
  harness will execute — they change how malformed requests are explained,
  applied, and budgeted.
- Tolerance is conservative by construction: when the engine is not confident
  where a hunk belongs, it returns `ContextNotFound`/`ContextMismatch` (with a
  re-read hint) rather than applying to a best guess.

## Testing Decisions

A good test here asserts **external behavior through the module's public
interface**, not internals: feed an input (a diff string, a rejection cause) and
assert the observable result (applied text, a specific `PatchError` variant, a
specific corrective string). No assertions on private helpers or struct fields.

Dedicated new tests are specified for two modules (per the chosen scope):

- **Patch Engine (Module A).** Pure-function tests on string fixtures —
  `(original, diff) -> applied text` and `(diff) -> PatchError`. Cover: a
  well-formed patch applies; count derived from body when the header count is
  off; a hunk a few lines off its stated position is located and applied within
  the search window; context that genuinely doesn't match yields
  `ContextMismatch`/`ContextNotFound`; missing file headers, bare `@@`, a context
  line missing its leading space, binary, rename, and `/dev/null` diffs each
  yield their specific variant (hard reject). Prior art: the existing
  `#[cfg(test)] mod tests` in the action module, which already exercises
  validation with literal fixtures.
- **Fix-Hint mapping (Module B).** Table-style tests asserting each rejection
  cause (missing `diff`, missing `command`, and every `PatchError` variant) maps
  to its specific corrective message, including that `ContextMismatch` surfaces
  the current line and `InvalidContextMarker` names the line and the leading-space
  fix. Prior art: the same action tests module (string-level assertions on
  `ActionResult` diagnostics).

Explicitly **not** given dedicated new test suites (deselected for this PRD):

- **Action Budget + auto-resume (Module D).** Verified through the existing
  `FakeRuntime` end-to-end app/orchestrator tests, which already drive runs via
  control phrases in the prompt and assert `RunState` transitions — the
  established prior art for control-flow behavior. A control phrase that emits
  recoverable malformed actions exercises the separation and bounded resume
  without a new dedicated unit harness.
- **Runtime Action-Protocol brief (Module C).** Lower-risk prompt text; covered
  by existing runtime prompt-construction tests if a light assertion is added
  that the rendered brief contains the documented params and an `apply_patch`
  example. No new suite required.

## Out of Scope

- New action kinds or changes to the action schema beyond documenting existing
  `params`.
- Relaxing approvals, capabilities, or workspace read/write roots.
- Supporting file-creation/deletion or rename via `apply_patch` (write_file
  remains the path for new files / full rewrites).
- Binary patch support.
- A general three-way/merge patch algorithm; tolerance is bounded offset/context
  search, not conflict resolution.
- Dedicated unit suites for Modules C and D (see Testing Decisions).
- Publishing this PRD to an external issue tracker; this is a local document.

## Further Notes

- **Recommended sequence (from the plan): P1 → P2 → P4 → P3**, i.e. Module C
  (documented params) and Module B (corrective feedback) first — small, low-risk,
  and expected to eliminate most `continue` interruptions on their own — then
  Module D (no manual continue), then Module A's tolerance hardening (the largest
  effort). The engine extraction itself is a prerequisite refactor for the typed
  errors that B consumes, so the extraction lands early even though the *tolerance*
  rules are sequenced last.
- **Operational note (not a code fix):** the `type: test` vs `type: chore`
  context mismatch is the signature of **concurrent runs editing the same**
  `.compozy/tasks/<slug>/*` files. Running one agent per file-set avoids that
  entire class of mismatch; tolerance reduces but cannot fully eliminate it,
  since two runs editing the same lines is a real conflict.
- The harness has always behaved correctly by rejecting malformed actions — this
  work reduces how often the model produces them, explains them when they happen,
  applies them when it safely can, and stops charging recoverable mistakes
  against the work budget.
