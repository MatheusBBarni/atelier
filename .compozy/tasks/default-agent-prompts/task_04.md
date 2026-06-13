---
status: pending
title: Add Prompt Drift And Role-Boundary Tests
type: chore
complexity: medium
dependencies:
  - task_02
  - task_03
---

# Task 04: Add Prompt Drift And Role-Boundary Tests

## Objective

Add focused Rust tests that fail when the six core default prompt surfaces drift or lose required role-contract language.

## Context

The TechSpec requires built-in runtime defaults and generated starter instruction files to share one canonical prompt body for each core role: `orchestrator`, `explorer`, `fixer`, `reviewer`, `oracle`, and `consul`.

After task_02 centralizes and wires the built-in defaults, and task_03 wires generated starter instruction files, this task adds tests that protect both surfaces from future drift.

## Scope

- Add alignment tests for all six core roles.
- Add lightweight prompt-content tests for required contract language.
- Extend existing config or init tests where practical instead of adding broad end-to-end initialization coverage.
- Keep tests explicit about the six core roles so role additions or removals require intentional updates.

## Implementation Notes

1. Locate the repository's existing Rust test style around `src/config/mod.rs`, config loading, or init/starter generation.
2. Add a test helper or table for the six core roles if it keeps assertions concise.
3. Assert that each built-in role instruction exactly equals the generated starter instruction file content for the matching role.
4. Assert that each core prompt contains contract-first structured-output language, including the requested JSON or structured contract requirement.
5. Assert that each core prompt mentions harness action discipline for unavailable file, command, edit, or verification operations where relevant.
6. Assert that each core prompt mentions blocker reporting.
7. Add role-specific boundary assertions, such as:
   - `orchestrator`: no direct implementation, no repository inspection, no edits or commands.
   - `explorer`: read-only discovery, no edits, no modifying commands.
   - `fixer`: scoped edits through harness actions, verification evidence or explicit blocker.
   - `reviewer`: risk-first review, no edits, no implementation takeover.
   - `oracle`: answer only from available evidence, do not pretend to have unseen data.
   - `consul`: critique and trade-off analysis, do not execute the plan.

## Acceptance Criteria

- Tests cover `orchestrator`, `explorer`, `fixer`, `reviewer`, `oracle`, and `consul` explicitly.
- Tests fail if built-in default prompt text and generated starter instruction file text differ for any core role.
- Tests fail if required structured-output, harness-action, blocker, or role-boundary language is removed from the core prompts.
- Tests avoid brittle full-text snapshots except for equality between the two canonical prompt consumers.
- Existing runtime schemas, action contracts, and validation behavior are unchanged.

## Verification

Run targeted Rust tests for the config/init area if available. Otherwise run the repository's normal Rust verification flow:

```bash
cargo fmt --check
cargo test
```

If the repository requires the local `rtk` wrapper, run the same commands through `rtk`.

## Out Of Scope

- Do not add a broad prompt evaluation suite.
- Do not change JSON schemas, action request shapes, result shapes, or orchestrator decision shapes.
- Do not standardize disabled or non-core bundled agents.
- Do not rewrite existing project-owned starter instruction files outside generated test fixtures.
