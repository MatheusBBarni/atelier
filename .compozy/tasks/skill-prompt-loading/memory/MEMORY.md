# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

## Shared Decisions
- Task 03 app integration stores `RunDriveContext.submitted_prompt`,
  normalized `RunDriveContext.prompt`, and optional in-memory
  `SkillPromptContext`; runtime rendering is still reserved for task 04.
- Task 03 run records persist `submitted_prompt`, normalized `prompt`, and
  metadata-only `loaded_skills`; full `SKILL.md` bodies are not persisted.
- Task 04 renders `SkillPromptContext` only inside `App::runtime_request`
  through the existing `RuntimeRequest.prompt` field; `RuntimeRequest` shape
  remains unchanged.
- Task 04 action authorization uses normalized `RunDriveContext.prompt` instead
  of rendered runtime prompt text so skill bodies cannot authorize VCS actions.
- Task 05 TUI skill suggestions consume shared `src/skills/mod.rs` metadata;
  `.multiagent/skills-cache.json` remains a TUI-owned advisory metadata cache
  and is not used for app-side skill resolution.

## Shared Learnings
- `cargo-llvm-cov v0.8.7` and the Rust `llvm-tools-preview` component were installed locally during task 01. Future tasks can use `cargo llvm-cov` for scoped coverage checks.
- Task 02 parser treats slash and backslash after `/skill:` as delimiter punctuation, so path-like text stops the skill id instead of becoming part of it.
- Task 02 `LoadedSkillMetadata.source_path` uses the canonical resolver path string (for example `.agents/skills/name/SKILL.md` or `~/.agents/skills/name/SKILL.md`), not an absolute filesystem path.
- Task 02 `render_runtime_prompt` escapes section delimiter lookalikes inside skill and user text; later runtime integration should call it once at request construction instead of pre-rendering into stored prompt/history data.
- Task 03 records `skills_loaded` after `run_started` and `prompt_submitted`
  for normal prompts, and after `subtask_started` for subtask runs, before
  runtime work begins.
- Task 04 keeps fake-runtime fixture routing deterministic by matching trigger
  words against the rendered `<User Prompt>` section only when the prompt has the
  skill-rendered `<System Prompt>` envelope.
- Task 05 shared skill suggestions suppress lower-precedence duplicate aliases
  so project roots beat personal roots and `.agents/skills` beats
  `.claude/skills` in autocomplete as well as resolver behavior.

## Open Risks

## Handoffs
