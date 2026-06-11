# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Implement task 04 by rendering loaded `SkillPromptContext` only at
  `RuntimeRequest` construction, threading the existing context through
  orchestrator, specialized-agent, parallel-child, council, and subtask runtime
  requests, and proving rendered skill bodies do not leak into persisted data or
  action authorization.

## Important Decisions
- Added a private `RuntimePrompt` carrier so `App::runtime_request` receives
  normalized prompt text plus optional `SkillPromptContext` without expanding
  the `RuntimeRequest` contract.
- `ActionExecutionContext.user_prompt` is now populated from
  `RunDriveContext.prompt` for normal and parallel action loops, not from the
  rendered runtime request prompt.
- Fake runtime trigger-word routing now inspects the rendered `<User Prompt>`
  section only when the prompt starts with the skill-rendered
  `<System Prompt>` envelope; raw prompts still use the full prompt text.

## Learnings
- `AGENTS.md` and `CLAUDE.md` were requested by the task prompt but are not
  present anywhere under this checkout.
- `cargo test` initially hit transient `runtime::codex` failures during the
  full suite; rerunning `runtime::codex::tests::` passed, and subsequent full
  verification passed.

## Files / Surfaces
- `src/app/mod.rs`: runtime-boundary skill rendering, action authorization
  prompt source, and app-level regression tests for all runtime prompt shapes.
- `src/runtime/fake.rs`: fake-runtime control text extraction for rendered skill
  envelopes.
- `tests/skill_prompt_loading.rs`: runtime-envelope placement and duplicate
  render regression coverage.

## Errors / Corrections
- Clippy rejected adding `skill_context` as an eighth `runtime_request`
  argument; replaced it with the private `RuntimePrompt` helper.
- Tightened fake runtime user-prompt extraction to require the rendered
  `<System Prompt>` prefix so raw prompts that mention `<User Prompt>` are not
  misinterpreted.

## Ready for Next Run
- Final verification evidence for task 04: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, and
  `cargo llvm-cov --summary-only` all passed. Coverage summary: 89.22% region,
  90.46% line.
