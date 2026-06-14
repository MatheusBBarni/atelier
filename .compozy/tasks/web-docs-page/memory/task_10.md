# Task Memory: task_10.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Build `src/docgen` + `atelier --emit-docs <dir>` (mirrors `--codemap`): deterministic Markdown reference fragments (`configuration.md`, `cli.md`) with `{title, nav_order}` frontmatter matching the docs collection schema. DONE.

## Important Decisions
- `configuration.md`: one canonical TOML fence built from `build_printable_config` (byte-identical to `--print-config`) + a `### <Name> — `[section]`` heading per section so the literal `[workspace]`/`[ui]`/`[limits]`/`[council]`/`[runtimes]`/`[agents]` tokens appear for the coverage test (full TOML only yields `[runtimes.x]`/`[agents.x]`, never bare `[agents]`).
- `cli.md`: walk `Cli::command().get_arguments()` for long flags; gate the Value column on `arg.get_action().takes_values()` (clap reports a default `<UPPER>` value-name even for bool flags). Slash table from `catalog()`; map `SlashCommandKind` → TUI/app/prompt prefix.
- `escape_table_cell` escapes `|`→`\|` — `/goal`'s usage `/goal | /goal <text>` would break the Markdown table otherwise.
- Frontmatter title is quoted (`title: "CLI & Commands"`) so `&` parses as a plain string under the zod schema (task_11 consumer).
- Dispatch placed after `--print-config` (needs merged `config`); guards: added `emit_docs` to the `--update` OR-chain and the `--codemap` exclusion, plus a dedicated `--emit-docs` exclusion bail.

## Learnings
- `build_printable_config` + the `Printable*` structs are `pub(crate)`; `docgen` reads them directly (no `Serialize` on slash types, per ADR-003).
- Determinism test asserts byte-identical across two output dirs; the live-runtime/`"doctor"` word scan must target only `configuration.md` — `cli.md` legitimately documents the `--doctor` flag.

## Files / Surfaces
- New: `src/docgen/mod.rs` (entry + renderers + 6 unit tests).
- `src/lib.rs`: `pub mod docgen;`.
- `src/cli.rs`: `emit_docs: Option<PathBuf>` field, guards, dispatch, 2 updated `Cli` literals, 3 new integration tests (`cli_with_emit_docs` helper).

## Errors / Corrections
- First determinism test forbade the word "doctor" across both fragments → false-failed on the `--doctor` flag row in `cli.md`. Scoped the forbidden-word scan to `configuration.md`.

## Ready for Next Run
- task_11 consumes `configuration.md` + `cli.md` from `web/src/content/docs/_generated/`. Generate with `atelier --emit-docs <dir>`; nav_order 30 (Configuration) / 40 (CLI & Commands).
- Pre-existing env-only test failures on this machine: `runtime::codex/claude/cursor` (codex version assertion + child-process timeouts) — not regressions.
