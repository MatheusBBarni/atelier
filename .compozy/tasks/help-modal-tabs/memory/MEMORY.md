# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State
- task_01 complete: `HelpTab` + `RosterRowStyle` enums live in `src/tui/mod.rs` (just before
  the `TuiCommand` enum, ~`:91`). Both are `#[allow(dead_code)]` until downstream tasks wire them.

## Shared Decisions
- New foundational TUI types are staged with `#[allow(dead_code)]` + a doc-comment naming the
  consuming task, since the crate has no `deny(warnings)` and the lib build would warn otherwise.
  Remove the allow when the consumer lands.

## Shared Learnings
- `src/tui/mod.rs` test module is at `:4099` (`#[cfg(test)] mod tests { use super::*; ... }`).

## Open Risks

## Handoffs
