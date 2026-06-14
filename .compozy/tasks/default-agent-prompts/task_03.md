---
status: completed
title: Align generated starter instruction files
type: refactor
complexity: medium
dependencies:
  - task_02
---

# Task 03: Align generated starter instruction files

## Objective

Update the starter instruction generation surface so newly initialized projects receive the same six core role-contract prompts used by the built-in runtime defaults.

## Context

The TechSpec identifies `src/config/mod.rs` as the relevant implementation area. The init flow generates starter instruction file contents through `starter_instruction_files`, while built-in defaults are populated through `insert_builtin_agent(...)`.

Task 02 centralizes and wires the six canonical prompt constants for built-in runtime defaults. This task reuses those same constants for generated starter instruction files so the two prompt surfaces cannot drift when new projects are initialized.

Core roles in scope:

- `orchestrator`
- `explorer`
- `fixer`
- `reviewer`
- `oracle`
- `consul`

## Requirements

- Update `starter_instruction_files` or its nearby helper logic to use the canonical Rust prompt constants for all six core starter instruction files.
- Preserve the existing starter file names, paths, and generated directory structure.
- Preserve existing starter config compatibility and init behavior outside prompt text wiring.
- Do not rewrite or migrate existing project-owned instruction files.
- Leave disabled or non-core bundled agents unchanged unless a small mechanical adjustment is required by the shared helper shape.

## Implementation Notes

- Prefer the existing local generation pattern in `src/config/mod.rs` over introducing a new configuration subsystem.
- If Task 02 introduced a helper map, role enum, or function for canonical prompt lookup, reuse that helper rather than duplicating match arms with raw prompt text.
- Keep the canonical prompt body in one Rust source of truth; starter generation should reference the constants or a helper that returns those constants.
- Be careful not to change runtime schemas, action contract shapes, permission enforcement, scheduling, parsing, validation, or retry mechanics.

## Acceptance Criteria

- Generated starter instruction files for all six core roles use the same canonical prompt bodies as the built-in defaults.
- The starter generation output still uses the existing instruction file naming and layout.
- Existing non-prompt starter config behavior is unchanged.
- Existing user-edited project instruction files are not touched by this task.

## Verification

Run targeted formatting and tests after the implementation tasks are complete:

- `cargo fmt --check`
- `cargo test` or a narrower config/init test target if available

If the repository requires the `rtk` wrapper, run the same commands through `rtk`.

## Completion Notes

Changed file: `src/config/mod.rs` only.

The six core-role tuples in `starter_instruction_files()` now reference the canonical
prompt constants introduced by Task 02 instead of separate inline literals:

- `("orchestrator", DEFAULT_ORCHESTRATOR_INSTRUCTIONS)`
- `("explorer", DEFAULT_EXPLORER_INSTRUCTIONS)`
- `("oracle", DEFAULT_ORACLE_INSTRUCTIONS)`
- `("consul", DEFAULT_CONSUL_INSTRUCTIONS)`
- `("fixer", DEFAULT_FIXER_INSTRUCTIONS)`
- `("reviewer", DEFAULT_REVIEWER_INSTRUCTIONS)`

Because both the built-in defaults (`insert_builtin_agent`) and the generated starter
instruction files now reference the same `&'static str` constants, the two prompt surfaces
share one source of truth and cannot drift for any core role.

Preserved unchanged:
- Starter file names / paths: each tuple's first element is still the role name and the
  init loop still writes `<config_dir>/agents/{role}.md` (consumer loop untouched).
- The out-of-scope tuples `librarian`, `designer`, `council-architect`, `council-security`,
  and `council-reviewer` keep their existing inline text.
- `write_private_file_if_missing` still skips already-existing files, so user-edited
  project-owned instruction files are never overwritten or migrated.
- No runtime schema, action contract, permission, scheduling, parsing, validation, or
  retry behavior was modified — only instruction-text wiring.

### Verification evidence

- `rtk cargo fmt --check` → exit 0 (no formatting changes needed).
- `rtk cargo test --lib config` → 56 passed, 0 failed (crate compiles; existing
  config/init tests unaffected). Dedicated alignment + drift tests are added in Task 04.
