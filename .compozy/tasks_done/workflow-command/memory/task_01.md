# Task Memory: task_01.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Implement task 01: recognize `/workflow <prompt>`, reject empty workflow prompts, and preflight Parallel Step Group availability before any Run/history creation.
- Implementation, verification, tracking updates, and local implementation commit `a86b94e` are complete.

## Important Decisions
- Keep task 01 scoped to parsing, preflight, diagnostics, unknown-command text, and focused app tests. Later workflow start events, prompt envelopes, ledgers, and chat projection remain for downstream tasks.
- Place workflow handling in `App::submit_prompt` after pending clarification answer handling and before `reject_unknown_slash_command`, matching the TechSpec and preserving slash-prefixed clarification answers.
- For enabled prerequisites, task 01 submits the extracted workflow prompt through the existing normal Run path while preserving the original `/workflow ...` text as `submitted_prompt`; downstream workflow tasks can replace the runtime prompt behavior with the workflow envelope.

## Learnings
- `AGENTS.md` and `CLAUDE.md` are not present in this checkout (`rg --files -g 'AGENTS.md' -g 'CLAUDE.md'` returned no files).
- Plain `fake_config` leaves `features.parallel_step_groups` disabled by default; workflow preflight can use it for the disabled-feature failure path.
- Existing `fake_parallel_config` enables `parallel_step_groups` with `max_parallel_agent_steps = 2`; tests can mutate `config.limits.max_parallel_agent_steps = 0` for the zero-limit path.
- Baseline source state rejects slash commands through `reject_unknown_slash_command`, whose available-command text did not include `/workflow <prompt>`.
- Verification evidence: focused `cargo test workflow_command -- --nocapture`, slash/prefix regression filters, `cargo fmt --check`, `cargo test --locked` after rerunning the known flaky Codex stdout test, `cargo clippy --all-targets -- -D warnings`, and `cargo llvm-cov --locked --summary-only` all passed. Coverage summary reported 90.67% line coverage.

## Files / Surfaces
- Expected implementation surface: `src/app/mod.rs`.
- Expected tracking surfaces after verification: `.compozy/tasks/workflow-command/task_01.md` and `.compozy/tasks/workflow-command/_tasks.md`.
- Touched implementation surface: `src/app/mod.rs`.
- Touched workflow memory surface: `.compozy/tasks/workflow-command/memory/task_01.md`.

## Errors / Corrections
- Initial submit-path wiring parsed and preflighted `/workflow` but still called `reject_unknown_slash_command` on the original slash input. Corrected by skipping unknown-command rejection once `parse_workflow_command` returns `Some`.
- First full `cargo test --locked` run failed only in known flaky `runtime::codex::tests::codex_adapter_emits_stdout_before_process_exit` with `Elapsed(())`; direct rerun passed, then the full suite passed.

## Ready for Next Run
- Task 02 should build on `WorkflowCommand { original_command, prompt }` and `handle_workflow_command` in `src/app/mod.rs` to add the workflow prompt envelope and `workflow_started` event.
