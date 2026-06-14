# Task Memory: task_05.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Concepts page (explanation, not reference): run loop, lifecycle states, routing, profiles vs runtimes, control-plane summary. DONE.

## Important Decisions
- `nav_order: 2` (after Quickstart=1, before Governance/Config which land later).

## Learnings
- `RunState` (src/orchestrator/mod.rs:14-23) = Idle, Planning, Running, WaitingForUser, Interrupted, Completed, Failed, LimitReached — only the 4 terminals + WaitingForUser are non-Running; used verbatim.
- Routing = `DecisionNextStep::{SingleAgent, ParallelGroup}` + `council` workflow target (COUNCIL_WORKFLOW_AGENT_ID, high-risk/explicit only).
- `RuntimeKind` (src/config/mod.rs:213-219) = codex, claude, cursor, zai, fake.

## Files / Surfaces
- Created `web/src/content/docs/concepts.md`.

## Errors / Corrections

## Ready for Next Run
- Onward links `../governance/` + `../configuration/` 404 until task_06/Wave-2 pages land (expected; lychee gate task_08 will catch only if still missing).
