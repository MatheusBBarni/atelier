# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Implement task 02 only: shared `src/skills/mod.rs` prompt parsing, resolution, dedupe, diagnostics, in-memory skill loading, and runtime prompt rendering. App, runtime adapter, history, chat projection, and TUI integration are explicitly deferred to later tasks.

## Important Decisions
- Use the task 01 shared discovery/YAML metadata foundation instead of adding a second resolver path.
- Treat missing `AGENTS.md` and `CLAUDE.md` in this checkout as unavailable repo guidance; `CONTEXT.md`, PRD, TechSpec, ADRs, task file, and workflow memory are the active guidance sources.
- Leave pre-existing task 01 tracking edits intact and separate from task 02 code changes.
- Parser delimiter set includes whitespace plus common punctuation, including slash and backslash, so `/skill:path/name` resolves `path` and leaves `/name` in the normalized prompt.
- `render_runtime_prompt(None, prompt)` returns the prompt unchanged; skill sections are emitted only when a loaded context exists.
- Loaded skill content is the instruction body after YAML frontmatter, not the frontmatter itself. Empty post-frontmatter bodies fail with `MissingContent`.

## Learnings
- Baseline `cargo test skills::compile_prompt -- --nocapture` ran zero tests because `compile_prompt` and task 02 resolver coverage do not exist yet.
- First full `cargo test` run failed in three Codex runtime tests (`codex_adapter_emits_stdout_before_process_exit`, `codex_availability_is_unknown_when_exec_env_auth_is_present`, and `codex_availability_is_unknown_when_login_status_is_unsupported`). All three passed when rerun individually, and the second full `cargo test` passed.
- Scoped coverage command used for this task: `cargo llvm-cov --lib --json --summary-only --output-path target/llvm-cov/skill-prompt-loading-summary.json -- skills::`.

## Files / Surfaces
- Expected code surface: `src/skills/mod.rs`.
- Expected test surface: skills unit tests and integration-oriented renderer tests.
- Implemented code surface: `src/skills/mod.rs`.
- Implemented test surface: `src/skills/mod.rs` unit tests and `tests/skill_prompt_loading.rs` integration-oriented renderer/metadata tests.

## Errors / Corrections
- Fixed clippy `unnecessary_sort_by` by using `scored.sort()` for suggestion ranking.
- Added explicit `Vec<LoadedSkill>` annotation in resolver dedupe after Rust could not infer the type from indexed metadata mutation.

## Ready for Next Run
- Final verification evidence after tracking updates: `git diff --check` passed; `cargo fmt --check` passed; `cargo clippy --all-targets --all-features -- -D warnings` passed; `cargo test` passed 332 lib tests, 4 CLI tests, 2 skill prompt tests, 2 skills foundation tests, and 4 ignored live runtime tests; `cargo llvm-cov --lib --json --summary-only --output-path target/llvm-cov/skill-prompt-loading-summary.json -- skills::` passed 26 focused skills tests and reported `src/skills/mod.rs` line coverage 1055/1164 (90.64%) and function coverage 99/108 (91.67%).
