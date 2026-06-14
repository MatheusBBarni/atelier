---
status: completed
title: Implement Role-Contract Default Prompts
type: refactor
complexity: medium
dependencies:
  - task_01
---

# Task 02: Implement Role-Contract Default Prompts

## Objective

Rewrite the six core built-in agent prompts as concise, contract-first role contracts while preserving existing runtime schemas, action contracts, permissions, model defaults, and agent enablement behavior.

## Context

The TechSpec identifies `src/config/mod.rs` as the implementation surface for built-in agent defaults defined through `insert_builtin_agent(...)`. The current default prompts need to be replaced with explicit role contracts for `orchestrator`, `explorer`, `fixer`, `reviewer`, `oracle`, and `consul`.

Task 01 should establish the canonical source shape for these prompt bodies. This task consumes that source and wires the built-in defaults for the six core agents to it.

## Requirements

- Implement or update the canonical default prompt body for each core role:
  - `orchestrator`
  - `explorer`
  - `fixer`
  - `reviewer`
  - `oracle`
  - `consul`
- Each prompt must require the agent to obey the runtime-requested structured output contract.
- Each prompt must explicitly forbid prose outside the requested JSON envelope when structured output is required.
- Each prompt must describe role ownership, role boundaries, harness action discipline, result discipline, blocker handling, and stop conditions.
- Built-in defaults for the six core agents must reference the canonical prompt source instead of separate inline text.
- Do not change runtime JSON schemas, action contract shapes, scheduler behavior, parser behavior, permission checks, model defaults, capabilities, or enablement behavior.
- Leave non-core and disabled bundled agents unchanged unless a small mechanical compile fix is required.

## Implementation Notes

- Keep prompt text concise and operational. The goal is reliable structured-runtime behavior, not broad agent philosophy.
- Prefer stable wording that tests can check with targeted phrase assertions without relying on brittle snapshots.
- Role boundaries should be explicit:
  - Orchestrator plans, routes, clarifies, and delegates; it must not inspect the repository, edit files, run commands, or do specialist work directly.
  - Explorer performs read-only discovery; it must not edit files or run modifying commands.
  - Fixer performs scoped implementation and verification; it must not claim direct tool access or complete without verification evidence or a specific blocker.
  - Reviewer performs risk-first review; it must not edit files or take over implementation.
  - Oracle answers focused questions from available evidence; it must not pretend to have unseen data.
  - Consul critiques plans and assumptions; it must not execute the plan or add unnecessary process overhead.

## Suggested Steps

1. Locate the built-in agent default definitions in `src/config/mod.rs`.
2. Replace the six core built-in prompt literals with references to the canonical prompt constants or helper introduced by Task 01.
3. Draft the six prompt bodies using the role contracts from the TechSpec.
4. Confirm the `insert_builtin_agent(...)` calls still preserve the same agent IDs, names, capabilities, enabled state, models, and runtime configuration.
5. Run formatting after edits if available in the current workflow.

## Acceptance Criteria

- The six core built-in agents use the centralized prompt text for their role.
- The default prompt for each core role contains explicit structured-output and JSON-envelope discipline.
- The default prompt for each core role contains blocker reporting guidance.
- The default prompt for each core role describes how to request harness actions when file, command, edit, or verification operations are unavailable directly.
- Non-prompt built-in agent configuration remains behaviorally unchanged.

## Verification

- Run `cargo fmt --check` or the repository's wrapper equivalent.
- Run the focused config tests if already available after Task 04, or defer full test verification to Task 05 if tests have not been added yet.
- Record any command failure as a blocker with the failing command and relevant error summary.

## Completion Notes

Added six module-level canonical prompt constants in `src/config/mod.rs` immediately
before `impl MergedConfig` (the single Rust source of truth required by the TechSpec):

- `DEFAULT_ORCHESTRATOR_INSTRUCTIONS`
- `DEFAULT_EXPLORER_INSTRUCTIONS`
- `DEFAULT_FIXER_INSTRUCTIONS`
- `DEFAULT_REVIEWER_INSTRUCTIONS`
- `DEFAULT_ORACLE_INSTRUCTIONS`
- `DEFAULT_CONSUL_INSTRUCTIONS`

Each constant is a concise role contract covering: role ownership, role boundaries
(forbidden behavior), harness-action discipline, result discipline, blocker handling,
and a stop condition. Every prompt contains the shared contract language
("structured output contract", "no prose outside the requested JSON envelope",
"harness action", a lowercase "blocker" reporting clause, and a "Stop …" condition)
plus a role-defining boundary phrase (orchestrator: no repository inspection / edits /
commands; explorer: read-only; fixer: verification evidence or specific blocker;
reviewer: no edits / no implementation takeover; oracle: do not pretend to have unseen
data; consul: do not execute the plan).

The six core `insert_builtin_agent(...)` calls now reference these constants instead of
inline literals. Capabilities, models, runtimes, effort, thinking, and `enabled` state
were left byte-for-byte unchanged. The disabled `librarian` and `designer` agents and all
runtime schemas, action contracts, parser, scheduler, and permission logic are untouched.

Starter instruction file alignment (`starter_instruction_files`) is deliberately deferred
to Task 03, and drift/role-boundary tests to Task 04, per the approved sequence.

### Verification evidence

- `rtk cargo fmt --check` → exit 0 (no formatting changes needed).
- `rtk cargo test --lib config` → 56 passed, 0 failed (crate compiles; existing config
  defaults/starter tests unaffected). Prompt-content/alignment assertions are added in
  Task 04.
