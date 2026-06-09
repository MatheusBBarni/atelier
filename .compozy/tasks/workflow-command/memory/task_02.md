# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Implement task 02: after task-01 workflow preflight, start one normal Run while preserving the raw `/workflow <prompt>` command in visible history, adding a `workflow_started` event, and sending a workflow prompt envelope to the runtime path.
- Acceptance gates include focused app tests for event ordering, raw prompt preservation, runtime-facing envelope content, normal prompt regression, and coverage >=80%.

## Important Decisions
- Keep task 02 scoped to startup metadata and runtime prompt shaping. Target ledger and workflow completion evidence remain for later tasks.
- Preserve the app's existing prompt compiler for the extracted workflow body, then split visible history from runtime prompt: `prompt_submitted.payload.prompt` and `RunDriveContext.submitted_prompt` keep the raw command, while `RunDriveContext.prompt` carries the workflow envelope.
- Add a small `WorkflowStart` plus `WorkflowPreflight` startup context in `src/app/mod.rs`; do not add `WorkflowRunContext` yet because the target ledger is task 03 scope.

## Learnings
- Baseline focused test `cargo test workflow_command_with_enabled_parallel_prerequisites_submits_extracted_prompt -- --nocapture` passes, confirming task-01 behavior reaches the fake runtime but currently records the extracted prompt as `prompt_submitted.payload.prompt`.
- Root `AGENTS.md` and `CLAUDE.md` are absent in this checkout; no additional repo guidance files were found by `rg --files -g 'AGENTS.md' -g 'CLAUDE.md'`.
- First full `cargo test --locked` run hit Codex runtime flake/env-interference failures; targeted reruns of the three Codex tests passed, and the required full locked suite passed on rerun.
- Verification evidence after implementation: `cargo fmt --check`, `cargo test workflow -- --nocapture`, rerun `cargo test --locked`, `cargo clippy --all-targets -- -D warnings`, and `cargo llvm-cov --locked --summary-only` all passed. Coverage summary reported 90.73% line coverage.

## Files / Surfaces
- Expected implementation surface: `src/app/mod.rs`.
- Expected tracking surfaces after verification: `.compozy/tasks/workflow-command/task_02.md` and `.compozy/tasks/workflow-command/_tasks.md`.
- Touched implementation surface: `src/app/mod.rs`.
- Touched workflow memory surface: `.compozy/tasks/workflow-command/memory/task_02.md`.

## Errors / Corrections
- Removed an unused `mut` surfaced by the focused workflow test compile before running clippy.

## Ready for Next Run
- Task 03 should build on the prompt split in `src/app/mod.rs`: workflow runs already have raw command history plus runtime envelope, but no app-owned target ledger or `WorkflowRunContext` field has been added yet.
