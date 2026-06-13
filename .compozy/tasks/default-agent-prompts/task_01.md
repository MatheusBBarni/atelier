---
status: pending
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
