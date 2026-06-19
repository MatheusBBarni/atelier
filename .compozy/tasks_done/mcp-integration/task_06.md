---
status: completed
title: Apply record-time redaction at event write
type: backend
complexity: medium
dependencies:
  - task_05
---

# Apply record-time redaction at event write

## Overview
Today redaction is display-only, but event payloads persist full-size to `.atelier/`, so a secret in an MCP tool result would be written to disk in cleartext. This task moves secret redaction to **record time** — before durable persistence — so the on-disk audit log can never become a credential honeypot. It is a security release-blocker for the PRD's "0 secret leaks" metric.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST redact secrets from event payloads BEFORE they are persisted (intercept at the durable-write path), not only in the chat projection.
- MUST reuse and, where needed, extend the existing redaction patterns (Bearer tokens, `sk-`, `zai-`) so MCP tool-call/result payloads are covered.
- MUST apply to the full payload, including nested MCP result content and error/diagnostic strings.
- MUST NOT alter the in-memory `ActionResult` used for execution control flow — redaction is for the persisted record.
- SHOULD record a structured flag indicating redaction was applied, for observability.
</requirements>

## Subtasks
- [x] 6.1 Identify the single durable-write choke point for events.
- [x] 6.2 Apply full-payload redaction there before serialization.
- [x] 6.3 Ensure MCP tool-call/result event payloads are covered by the patterns.
- [x] 6.4 Add an observability flag for redaction-applied.

## Implementation Details
Modify `src/history/mod.rs` `append_event` (the persistence choke point) to redact the payload before `serde_json::to_writer`, reusing the redaction helpers in `src/runtime/mod.rs`. Confirm the MCP event payloads produced in task_05 flow through this path. Do not change display-time projection. See TechSpec "Technical Considerations → Key Decisions (record-time redaction)" and ADR-006 implementation notes.

### Relevant Files
- `src/history/mod.rs` — `append_event` (~140), the durable-write point.
- `src/runtime/mod.rs` — `redact_sensitive_text` and helpers (~704) to reuse/extend.
- `src/app/mod.rs` — `record_event_with_group` (~4215) which calls `append_event`.

### Dependent Files
- `src/actions/mcp_handlers.rs` (task_05) — produces the MCP result payloads being redacted.

### Related ADRs
- [ADR-006: Persist MCP trust and description pins in an app-managed .atelier/ store](../adrs/adr-006.md) — notes record-time redaction as the data-handling rule.

## Deliverables
- Record-time redaction applied at the event-write choke point.
- MCP payloads covered by the redaction patterns.
- Redaction-applied observability flag.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test proving no secret reaches disk **(REQUIRED)**.

## Tests
- Unit tests (`src/history/mod.rs`):
  - [x] A payload containing `Bearer abc123` is redacted before serialization. (`redact_json_redacts_bearer_token`)
  - [x] A payload containing an `sk-…` token in a nested MCP result `content` field is redacted. (`redact_json_redacts_nested_sk_token_in_mcp_content`)
  - [x] A payload with no secrets is written unchanged. (`redact_json_leaves_clean_payload_unchanged` + `append_event_with_no_secret_is_written_unchanged`)
- Integration tests (`src/history/mod.rs`, real write→read to `.atelier/`):
  - [x] An MCP tool result carrying a known injected secret is recorded, and reading the on-disk `.atelier/` events file shows the secret redacted (not present in cleartext). (`append_event_redacts_secret_on_disk_but_not_in_memory`)
  - [x] The in-memory `ActionResult`/event used for control flow is unaffected by record-time redaction. (same test asserts `event.payload` still holds the secret)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Choke point.** Redaction is applied in `HistoryStore::append_event` (the single durable-write path every event flows through, including MCP `mcp_tool_result`/action-result events) and in `append_debug_event` (the debug log is on-disk too). The run-record and artifact writers are out of scope for this task.
- **Observability flag without a schema change.** Adding a `redaction_applied` field to `HistoryEvent` would ripple to ~8 struct-literal sites across modules; instead, when a secret is redacted the persisted payload object gains a `_redacted: true` key (a structured flag in the record, ignored by the chat projection). Secret-free events are written byte-for-byte unchanged (no flag, no clone), so existing read-back equality tests are unaffected.
- **In-memory untouched.** The caller's `&HistoryEvent` is never mutated; a redacted clone is serialized only when a secret was present, so execution control flow (the in-memory `ActionResult`) is unaffected.
- **Patterns reused.** Redaction walks the full payload JSON recursively and applies `crate::runtime::redact_sensitive_text` (Bearer / `sk-` / `zai-`) to every string, covering nested MCP result content and diagnostic strings.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- 4 redaction unit tests + 1 on-disk integration test: pass (49 history tests total).
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1323 passed, 4 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- A known secret in an MCP payload never appears in the durable `.atelier/` history.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
