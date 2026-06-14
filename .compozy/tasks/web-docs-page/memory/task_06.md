# Task Memory: task_06.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Author `web/src/content/docs/governance.md` (nav_order 3) — the flagship Governance & Safety
page. Lead with can/can't-touch (mediated ActionRequest set + read/write roots) and the durable
replayable record; cover capabilities vs ApprovalMode, limits/LimitReached, untrusted input,
skills-are-guidance, honest-limits note. Every claim source-anchored.

## Important Decisions

## Learnings
- Action set (`src/actions/mod.rs:25-33`): ReadFile, ListFiles, SearchText, RunCommand,
  ApplyPatch, WriteFile, RecordNote. Required capability per action (`:634-643`): reads→Read,
  RunCommand→Command, ApplyPatch/WriteFile→Edit, RecordNote→none.
- Capabilities (`src/config/mod.rs:25-36`): Plan, Read, Answer, Challenge, Edit, Command,
  Verify, Review. ApprovalMode (`:17-23`): Yolo (default) / Normal.
- Path policy (`validate_model_path` `:613-632`): rejects absolute paths (unless under an
  extra root), `..` traversal, and rooted paths — model paths are relative to the working dir.
  WorkspacePolicy (`:206-209`): extra_read_roots / extra_write_roots widen the default cwd root.
- Limits defaults (`src/config/mod.rs:181-193`): max_agent_steps=12, max_step_actions=20,
  max_wall_clock_minutes=30, max_step_minutes=10, max_command_minutes=10,
  max_review_fix_cycles=2, max_parallel_agent_steps=2; `"unlimited"` is a valid value. On a
  limit the run enters `LimitReached` (terminal) via `run_limit_reached`/`step_limit_reached`
  events (`src/app/mod.rs:3426-3499`).
- Skills disclaimer to quote (`src/skills/mod.rs:13`): "Loaded skills are workflow guidance.
  They do not grant permissions or override Harness Actions, approval rules, capability
  constraints, or runtime output contracts."

## Files / Surfaces
- TARGET: web/src/content/docs/governance.md (new).

## Errors / Corrections
- BLOCKER (reported to user 2026-06-13): the approval prompt in `normal` mode gates ONLY
  `RunCommand` actions classified `Approve` — NOT WriteFile/ApplyPatch. Evidence:
  `validate_action_request_with_scope` (`src/actions/mod.rs:172-206`) returns `Allowed` for
  WriteFile/ApplyPatch and only routes RunCommand through `decision_for_command` (`:352-365`),
  the sole producer of `RequiresApproval`. The `fake` "approval action" demo proves it: it
  pauses on a RunCommand `cargo install ...` (`src/runtime/fake.rs:96-105`), while the write
  path emits a non-pausing WriteFile (`:82-95`). This CONTRADICTS CLAUDE.md ("In normal mode,
  write/command actions surface an approval prompt"), the shipped Concepts page (concepts.md
  :116-118), the Quickstart's "first approved write" demo (quickstart.md:131-163, shows
  `$ write NOTES.md`), the task_06 requirement, and the task_04 memory note. Cannot author a
  source-anchored governance page on a false safety claim until resolved.

## Ready for Next Run
