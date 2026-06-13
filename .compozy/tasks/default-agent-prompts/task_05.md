---
status: pending
title: Generate and validate Compozy task artifacts
type: chore
complexity: low
dependencies:
  - task_01
  - task_02
  - task_03
  - task_04
---

# Task 05: Generate and validate Compozy task artifacts

## Objective

Validate that the default-agent-prompts task bundle is complete, internally consistent, and accepted by the repository's Compozy task validation flow.

## Context

The PRD, TechSpec, ADRs, task list, and implementation task files define the default-agent-prompts work. This final task closes the planning artifact generation step by checking that the generated task files match the approved five-task breakdown and satisfy the repository's task metadata requirements.

## Scope

- Verify `.compozy/tasks/default-agent-prompts/_tasks.md` lists all five tasks with the expected titles, status, complexity, and dependencies.
- Verify `task_01.md` through `task_05.md` exist under `.compozy/tasks/default-agent-prompts/`.
- Verify each task file contains required YAML frontmatter fields: `status`, `title`, `type`, `complexity`, and `dependencies`.
- Run the repository's Compozy task validation command for `default-agent-prompts`.
- Fix validation-only issues in generated task artifacts when the fix is mechanical and in scope.

## Out Of Scope

- Do not implement the Rust prompt changes in this task.
- Do not change PRD, TechSpec, or ADR decisions unless validation exposes a direct artifact inconsistency.
- Do not redesign the task breakdown.
- Do not modify unrelated `.compozy` task bundles.

## Inputs

- `.compozy/tasks/default-agent-prompts/_prd.md`
- `.compozy/tasks/default-agent-prompts/_techspec.md`
- `.compozy/tasks/default-agent-prompts/_tasks.md`
- `.compozy/tasks/default-agent-prompts/task_01.md`
- `.compozy/tasks/default-agent-prompts/task_02.md`
- `.compozy/tasks/default-agent-prompts/task_03.md`
- `.compozy/tasks/default-agent-prompts/task_04.md`
- `.compozy/tasks/default-agent-prompts/task_05.md`

## Implementation Steps

1. Inspect the generated task list and confirm it contains exactly the approved five tasks:
   - `task_01`: Model prompt source and generation surfaces
   - `task_02`: Implement role-contract default prompts
   - `task_03`: Align generated starter instruction files
   - `task_04`: Add prompt drift and role-boundary tests
   - `task_05`: Generate and validate Compozy task artifacts
2. Inspect each task file and confirm frontmatter is present and uses allowed task metadata values.
3. Confirm dependencies match the approved sequence:
   - `task_01`: no dependencies
   - `task_02`: depends on `task_01`
   - `task_03`: depends on `task_02`
   - `task_04`: depends on `task_02` and `task_03`
   - `task_05`: depends on `task_01`, `task_02`, `task_03`, and `task_04`
4. Run `compozy tasks validate --name default-agent-prompts` through the repository's normal command wrapper if required by the local environment.
5. If validation fails because of task artifact shape, metadata, missing files, or simple formatting, update only the generated task files and rerun validation.
6. If validation cannot run because the command or environment is unavailable, report the exact blocker and preserve the generated artifacts.

## Acceptance Criteria

- `_tasks.md` and `task_01.md` through `task_05.md` exist in `.compozy/tasks/default-agent-prompts/`.
- The task list and individual task files describe the same five-task implementation sequence.
- All task files include required frontmatter fields.
- Dependencies are explicit and match the approved breakdown.
- Compozy task validation passes, or a specific actionable blocker is reported.

## Verification

- Run `compozy tasks validate --name default-agent-prompts` using the repository's expected command wrapper when applicable.
- Record the validation command and result in the final agent result.
- If validation fails and is fixed, rerun the same validation command after the fix.

## Residual Risk

This task validates planning artifact structure, not the future Rust implementation. Implementation correctness is covered by the dependent implementation and test tasks.
