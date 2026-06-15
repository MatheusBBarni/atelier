---
status: pending
title: Add the McpTrustStore with durable trust tiers and pins
type: backend
complexity: medium
dependencies:
  - task_01
---

# Add the McpTrustStore with durable trust tiers and pins

## Overview
Make per-server trust durable and revocable. The `McpTrustStore` persists each server's trust tier and a pinned hash of every tool's `(name + description + input schema)` to a workspace-scoped `.atelier/mcp-trust.json`, and records promote/revoke as events. Validation (task_05) consults an in-memory cache of this store, and the diff-on-change pin defends against tool-description rug-pulls.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST persist per-server trust tier (`Untrusted`/`Trusted`) and per-tool pinned hashes to a workspace-scoped `.atelier/mcp-trust.json`.
- MUST expose `tier`, `pin`, `promote`, and `revoke`; `promote`/`revoke` MUST write the file and record an event (`mcp_server_trusted` / `mcp_server_revoked`).
- MUST load the store into an in-memory cache at startup for synchronous reads during validation.
- MUST compute the pin as a hash of `(tool name + description + input schema)` and detect a changed pin.
- MUST store only server ids and hashes — never secrets or full payloads.
- SHOULD treat the event log as the audit trail and the file as a rebuildable snapshot.
</requirements>

## Subtasks
- [ ] 4.1 Define the `McpTrustStore` type, `TrustTier`, and the on-disk JSON shape.
- [ ] 4.2 Implement load/save against the workspace `.atelier/` directory.
- [ ] 4.3 Implement `promote`/`revoke` with file write + event emission.
- [ ] 4.4 Implement pin computation and changed-pin detection.
- [ ] 4.5 Provide a synchronous `tier`/`pin` read path for validation.

## Implementation Details
Create `src/mcp/trust_store.rs`; declare it in `src/mcp/mod.rs`. Persist alongside existing per-session artifacts managed by `src/history/mod.rs` (the `ui_state.json` artifact at the history root is the precedent). Promote/revoke emit events via the existing `record_event` path. See TechSpec "Data Models" (Trust store JSON) and ADR-006.

### Relevant Files
- `src/history/mod.rs` — `HistoryStore` root + artifact write precedent (`ui_state.json`, ~73) for placing `mcp-trust.json`.
- `src/app/mod.rs` — `record_event_with_group` (~4215) to emit trust events.
- `src/mcp/mod.rs` — module declaration.

### Dependent Files
- `src/actions/mod.rs` (task_05) — validation consults `tier`/`pin`.
- `src/app/mod.rs` / `src/tui/mod.rs` (task_09) — promote/revoke from the approval card.

### Related ADRs
- [ADR-006: Persist MCP trust and description pins in an app-managed .atelier/ store](../adrs/adr-006.md) — the store's design.

## Deliverables
- `McpTrustStore` with durable, workspace-scoped persistence.
- Promote/revoke with event emission.
- Pin computation + changed-pin detection.
- In-memory cache for synchronous validation reads.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for persistence round-trip **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A newly seen server defaults to `Untrusted`.
  - [ ] `promote("fs")` then reload returns `Trusted` for `fs`.
  - [ ] `revoke("fs")` returns it to `Untrusted` and persists.
  - [ ] A tool whose description changes produces a different pin than the stored one (changed-pin detected).
  - [ ] The persisted file contains no secret values — only ids and hashes.
- Integration tests:
  - [ ] `promote` writes `.atelier/mcp-trust.json` and emits an `mcp_server_trusted` event in history.
  - [ ] A fresh store loaded from an existing file reflects prior promote/revoke decisions (cross-session remember).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Trust survives a restart, is revocable, and never stores secrets.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
