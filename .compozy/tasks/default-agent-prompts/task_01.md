---
status: completed
title: "Model prompt source and generation surfaces"
type: chore
complexity: medium
dependencies: []
---

# Task 01: Model prompt source and generation surfaces

## Objective

Map the existing default-agent prompt implementation surfaces so the shared prompt source can be introduced without changing unrelated runtime behavior.

## Context

The Default Agent Prompts TechSpec identifies `src/config/mod.rs` as the relevant implementation area. Current behavior is expected to include built-in agent defaults through `insert_builtin_agent(...)`, starter configuration text through `starter_config_text`, and generated starter instruction file contents through `starter_instruction_files`.

This task establishes the precise edit and test surface before the implementation tasks modify prompt text.

## Scope

- Inspect the current config implementation for built-in definitions of `orchestrator`, `explorer`, `fixer`, `reviewer`, `oracle`, and `consul`.
- Identify how generated starter instruction files are named, populated, and returned.
- Identify existing tests around config defaults, starter config generation, and starter instruction generation.
- Record the current relationship between built-in defaults and starter-generated prompt files.
- Leave runtime schemas, action contracts, scheduler behavior, parser behavior, and permission enforcement unchanged.

## Implementation Notes

- Prefer existing local helpers and test style over introducing a new abstraction in this task.
- If `src/config/mod.rs` is already large, note whether a nearby module would be cleaner for constants, but defer the actual extraction to Task 02.
- The output of this task should make Task 02 and Task 04 mechanically clear.

## Acceptance Criteria

- The locations of all six core built-in prompt definitions are identified.
- The starter instruction file generation surface for all six core roles is identified.
- Relevant existing tests or the absence of existing tests are identified.
- No source files are changed unless a small task note is needed by the repository's task workflow.
- The next task can implement centralized constants without rediscovering the config surface.

## Verification

- Run the narrowest useful read-only verification available for this task, such as a targeted search or existing config test discovery command.
- Do not run modifying commands.
- Report any command that could not run with the specific blocker.

## Expected Findings To Carry Forward

- Exact file path and function or test names for the built-in default prompt surface.
- Exact file path and function or test names for starter instruction file generation.
- Any existing tests that should be extended for drift and prompt-content assertions.

## Findings (Carry Forward)

All surfaces live in a single file: `src/config/mod.rs` (3278 lines). No nearby module
extraction is required; a constants block near the built-in definitions is sufficient for
Task 02. No conflicts found between this task spec, `_techspec.md`, and ADR-001/ADR-002.

### Built-in default prompt surface

- Helper: `insert_builtin_agent(...)` at `src/config/mod.rs:1541`.
- Input struct: `BuiltinAgent` at `src/config/mod.rs:1467`; prompt carried in
  `instructions: &'static str` (field decl line 1475), wired to
  `InstructionSource::Inline(agent.instructions.to_string())` at line 1553.
- Six core agents, each an independent inline string literal:
  | role | `insert_builtin_agent` block | `instructions:` line | enabled |
  |---|---|---|---|
  | orchestrator | 671 | 681 | true |
  | explorer | 686 | 696 | true |
  | oracle | 701 | 711 | true |
  | consul | 716 | 726 | true |
  | fixer | 732 | 747 | true |
  | reviewer | 752 | 767 | true |
- Out of scope (disabled, leave unchanged): `librarian` (773, enabled:false),
  `designer` (790, enabled:false).

### Starter instruction file generation surface

- `starter_instruction_files() -> Vec<(&'static str, &'static str)>` at
  `src/config/mod.rs:2252`. Tuple is `(role_name_used_as_filename, instruction_body)`.
- Consumed by the init flow at lines 2099–2102: for each tuple it writes
  `<config_dir>/agents/{name}.md` via `write_private_file_if_missing` (existing files are
  skipped, never overwritten — satisfies the "do not migrate project-owned files" rule).
- Contains the same six core roles as separate inline literals (orchestrator 2256,
  explorer 2260, oracle 2264, consul 2268, fixer 2272, reviewer 2276) plus out-of-scope
  `librarian`, `designer`, `council-architect`, `council-security`, `council-reviewer`.
- `starter_config_text()` at line 2111 generates the `atelier.toml` body only; it does not
  carry per-role instruction prose, so it is out of scope for the canonical prompt wiring.

### Built-in ↔ starter relationship (drift risk)

The two surfaces are **fully independent inline literals with no shared source**. Editing
one does not affect the other — this is the exact drift Task 02/03 close by introducing
canonical `DEFAULT_<ROLE>_INSTRUCTIONS` constants and pointing both `insert_builtin_agent`
calls and `starter_instruction_files()` at them.

### Existing tests (gap for Task 04)

- No existing test asserts prompt instruction *content* or built-in↔starter *alignment*.
- Closest config tests in the `#[cfg(test)] mod tests` block (starts line 2346):
  `builtin_config_resolves_without_files` (2362), `builtin_config_includes_opt_in_*runtime`
  (2382, 2398), `starter_config_includes_*runtime_without_protected_flags` (3184, 3197) —
  all assert runtime/limits, none assert instruction text.
- Task 04 should add new alignment + required-phrase tests in this same test module.

### Verification (read-only)

Targeted `grep`/`Read` discovery over `src/config/mod.rs` only; no modifying commands run,
per the task's read-only constraint.
