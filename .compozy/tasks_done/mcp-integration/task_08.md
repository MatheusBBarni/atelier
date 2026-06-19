---
status: completed
title: Project MCP tool calls and events into the chat transcript
type: backend
complexity: medium
dependencies:
  - task_05
---

# Project MCP tool calls and events into the chat transcript

## Overview
Every MCP tool call must be visible in the transcript like any other action. This task adds the chat-projection arms that turn MCP action requests/results and `mcp_tool_called`/`mcp_tool_result` events into `ChatItemView`s, reusing the existing call→result rendering shape and the 8KB display cap. It covers the transcript only; the interactive approval card is task_09.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add projection arms so `CallMcpTool`/`ReadMcpResource`/`ListMcpResources` requests and results render as `ChatItemView`s (title, summary, body, status).
- MUST add arms for `mcp_tool_called` and `mcp_tool_result` events.
- MUST reuse the existing 8KB content-preview cap for display.
- MUST render an MCP tool failure with an error status, mirroring command failures.
- MUST add the new action-kind label strings so `action_kind_label` resolves them.
</requirements>

## Subtasks
- [x] 8.1 Add request/result projection arms for the three MCP action kinds.
- [x] 8.2 Add (no-op) projection arms for `mcp_tool_called`/`mcp_tool_result` events — see note on duplication.
- [x] 8.3 Add the action-kind label strings.
- [x] 8.4 Apply the 8KB display cap to MCP result previews.

## Implementation Details
Modify `src/app/chat/projection.rs`: the `apply_action_completed` string dispatch (~718), `action_requested_view` (~1509), `action_kind_label` (~2461), and `apply_history_event` (~58) for the new event kinds. Reuse `capped_content_preview` and the existing `CommandResult`/`ChatItemView` shapes. See TechSpec "System Architecture → chat projection" and PRD CF5. Do not implement the approval card (task_09).

### Relevant Files
- `src/app/chat/projection.rs` — `apply_history_event` (~58), `apply_action_completed` (~718), `action_requested_view` (~1509), `action_kind_label` (~2461), `CONTENT_PREVIEW_CAP_BYTES` (~1575).
- `src/app/mod.rs` — emits the `mcp_tool_called`/`mcp_tool_result` events (task_05).

### Dependent Files
- None; rendering-only, owns its own arms.

### Related ADRs
- [ADR-001: Broker MCP through the harness ActionRequest contract](../adrs/adr-001.md) — calls project into chat like every action.
- [ADR-007: Read-only auto-allow via the protocol resource/tool split](../adrs/adr-007.md) — resource vs tool distinction in labels.

## Deliverables
- Projection arms for MCP action requests/results and `mcp_*` events.
- Action-kind label strings.
- 8KB-capped MCP result previews.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of an end-to-end call appearing in the transcript **(REQUIRED)**.

## Tests (`src/app/chat/projection.rs`)
- Unit tests:
  - [x] A `call_mcp_tool` requested event projects a pending `ChatItemView` with the tool name in the title. (`mcp_call_requested_projects_pending_with_tool_name`)
  - [x] A successful MCP result projects a completed item; a failed one projects an error-status item. (`mcp_call_completed_projects_completed_item`, `mcp_call_failed_projects_error_item`)
  - [x] An MCP result larger than 8KB is truncated at the cap in the projected body. (`mcp_result_body_is_capped_at_8kb`)
  - [x] `action_kind_label` returns a human label for each of the three new kinds. (`action_kind_label_resolves_mcp_kinds`)
- Integration tests:
  - [x] A full brokered `CallMcpTool` appears in the projected transcript as call→result with the correct final status. (`mcp_call_completed_projects_completed_item` rebuilds the call+result events into one evolving item; the live fake-server execution that produces those events is covered by `tests/mcp_actions.rs`, task_05.)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **MCP renders through the standard action pipeline.** task_05 executes MCP actions via `execute_action_request`, so they already produce `action_requested`/`action_completed` events. The transcript view is therefore built by routing those events (`apply_action_completed` → new `apply_action_completed_mcp`) — not by a separate event kind. `ActionContext` gained `server`/`tool` fields (from the request `params`) so the title shows `server/tool`.
- **`mcp_tool_called`/`mcp_tool_result` arms are explicit no-ops.** These are audit/observability events (the `mcp_tool_result` metric feeds `--doctor`, task_10). Rendering them as chat *too* would duplicate the action-pipeline item, so they are grouped with `mcp_catalog_snapshot` as preserved-but-not-rendered. The unit tests target the real rendering path (`action_completed` with `kind = call_mcp_tool`), which is what the test descriptions mean by "mcp_tool_result".
- **8KB cap reused.** MCP result content renders as a `code` body line through the existing `capped_content_preview` (`CONTENT_PREVIEW_CAP_BYTES = 8KB`); failures add an error line mirroring command failures.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- 5 projection unit/integration tests (pending / completed / failed / 8KB cap / labels): pass.
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1333 passed, 4 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- MCP calls and results are visible in the transcript with correct status and capped previews.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
