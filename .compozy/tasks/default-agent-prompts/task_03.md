---
status: pending
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

Report the changed files, the starter generation surface that now reuses the canonical constants, and any verification blocker if tests cannot be run.
