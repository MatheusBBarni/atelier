# Task Memory: task_03.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Update runtime prompt contracts (Codex, Claude, Cursor) and shared orchestrator guidance so `waiting_for_user` decisions describe `clarifying_options` (2-4) and `recommended_option_id`, with the custom-answer boundary stated. Completed 2026-06-10.

## Important Decisions

- The `waiting_for_user` rules block is identical verbatim across `codex_prompt_text`, `claude_prompt_text`, and `cursor_prompt_text` to prevent wording drift; tests assert the same phrases in all three.
- Shared orchestrator guidance expanded the existing routing rule line ("Ask a clarifying question...") into three lines rather than adding a new section, keeping `build_orchestrator_prompt` structure unchanged.
- Negative assertions use `!contains("question tool")` and `!contains("ask_user")` to guard the no-any-agent-question-tool requirement.

## Learnings

- Z.ai needs no prompt edit: `zai.rs` sends `agent_profile.instructions` as the system message, and `app/mod.rs:3137` sets orchestrator instructions from `build_orchestrator_prompt`, so the new guidance reaches Z.ai through the shared path.
- Inside the runtime prompt `format!` raw strings, literal JSON braces in the option-shape example must be doubled (`{{`/`}}`).

## Files / Surfaces

- `src/runtime/codex.rs` — decision example fields + waiting_for_user rules + `codex_prompt_text_describes_structured_clarification_contract` test.
- `src/runtime/claude.rs` — same pattern + test.
- `src/runtime/cursor.rs` — same pattern + test.
- `src/orchestrator/mod.rs` — three routing-rule lines in `build_orchestrator_prompt` + `generated_orchestrator_prompt_includes_structured_clarification_guidance` test.

## Errors / Corrections

- None. fmt/clippy/test clean on first full run (423 passed, 0 failed).

## Ready for Next Run

- Task complete and committed locally. Tasks 04+ can rely on the prompt-contract phrases asserted by the four `*_structured_clarification_*` tests if they need to reference exact guidance wording.
