# Parallel agents run as step groups inside one active run

The harness will support concurrent specialized agent work by adding parallel
step groups inside a single active run, not by allowing multiple active runs at
the same time. This preserves the existing prompt-to-run and TUI interaction
model while letting the Orchestrator start independent agent steps concurrently
when their file scopes are clear.

Parallel steps will run in the same working directory for the first
implementation, with harness-enforced disjoint file scopes instead of isolated
worktrees. This keeps the feature aligned with existing Harness Action,
Capability Enforcement, Action Approval, and Session History boundaries, while
making write safety depend on explicit scope validation.

The rejected alternatives were parallel active runs, isolated worktrees for
every parallel step, and a dynamic DAG scheduler that lets the Orchestrator
consume partial results before a group joins. Those options may become useful
later, but they would significantly change the run model, merge semantics, TUI
state, and cancellation behavior before the core parallel-agent workflow is
proven.
