# Task Memory: task_09.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Factor the inlined `EffectiveConfig → PrintableConfig` mapping out of `to_redacted_toml` into a reusable `build_printable_config`, expose `Printable*` at crate visibility for task_10's `docgen`, keep `--print-config` byte-identical.

## Important Decisions
- New `pub(crate) fn build_printable_config(&EffectiveConfig) -> PrintableConfig` holds the whole mapping; `to_redacted_toml` is now just `toml::to_string_pretty(&build_printable_config(config))`.
- Made all five `Printable*` structs AND their fields `pub(crate)` (not just the types) so docgen can read them to render Markdown — task subtask 9.3 ("pub(crate) or accessor"); field access is cheaper than accessors for the dependent task.
- `prompt_source_label` left as-is (free fn); redaction logic unchanged.

## Learnings
- `print_config_renders_toml` integration test lives in `src/cli.rs:188` (a `#[tokio::test]` in `cli::tests`), NOT under `tests/` — run via `cargo test --lib print_config_renders_toml`.
- `WorkspacePolicy` fields are `extra_read_roots`/`extra_write_roots` (no `read_paths`); `Limits` has `max_parallel_agent_steps` (u32) + several `Limit` enum fields (no `max_steps_per_run`).

## Files / Surfaces
- `src/config/mod.rs` only: `Printable*` structs (~:1792-1864), `build_printable_config` (~:1866), `to_redacted_toml` wrapper (~:1979), 2 new unit tests after `redacted_toml_contains_env_reference_not_secret`.

## Errors / Corrections
- Initial test referenced non-existent `limits.max_steps_per_run` / `workspace.read_paths`; corrected to real fields.

## Ready for Next Run
- task_10 (`src/docgen/`) can now `use crate::config::{build_printable_config, PrintableConfig, ...}` and read fields directly to emit `configuration.md`.
