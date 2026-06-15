---
status: pending
title: Extend the approval card with MCP description and trust controls
type: frontend
complexity: high
dependencies:
  - task_04
  - task_05
---

# Extend the approval card with MCP description and trust controls

## Overview
Turn the approval moment into the product's trust-legibility differentiator. When an untrusted MCP server first wants to act, the approval card shows the tool's full, untruncated description and origin server, and the user can promote the server to trusted (remembered, revocable) right there. This directly counters the "click-once, the model reads everything" tool-poisoning status quo.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST surface the MCP tool's full, untruncated description and its origin server id in the approval card for a `CallMcpTool` awaiting approval.
- MUST let the user promote the origin server to trusted from the approval flow, and MUST make that trust revocable.
- MUST emit `mcp_server_trusted` / `mcp_server_revoked` events and project them into the transcript.
- MUST integrate new key handling within the existing approval key-routing precedence without breaking approve/deny.
- MUST NOT auto-promote on a plain approve — promotion is an explicit, separate action.
- SHOULD show the trust tier (untrusted/trusted) in the card.
</requirements>

## Subtasks
- [ ] 9.1 Extend the pending-approval view with tool description, origin server, and trust tier.
- [ ] 9.2 Render those fields in the approval card.
- [ ] 9.3 Add key handling to promote (and revoke) server trust from the approval flow.
- [ ] 9.4 Persist promote/revoke via the trust store and emit events.
- [ ] 9.5 Project `mcp_server_trusted`/`mcp_server_revoked` into the transcript.

## Implementation Details
Extend the pending-approval view type and `apply_pending_approval` in `src/app/chat/projection.rs` to carry/render the MCP description, origin, and tier (populated from the validation reason produced in task_05). Add key routing in `src/tui/mod.rs` within the existing approval precedence. Wire promote/revoke to the `McpTrustStore` (task_04) in `src/app/mod.rs`. See TechSpec "User Experience" (PRD) and ADR-003/ADR-006/ADR-007.

### Relevant Files
- `src/app/chat/projection.rs` — `apply_pending_approval` (~192) approval-card rendering.
- `src/app/mod.rs` — `PendingApprovalView` fields, `resolve_pending_approval` (~1587), promote/revoke wiring.
- `src/tui/mod.rs` — approval key routing (precedence: help → clarification → approval → …).
- `src/mcp/trust_store.rs` — promote/revoke (task_04).

### Dependent Files
- None; owns its own trust-event projection arms.

### Related ADRs
- [ADR-003: Config-first MVP product surface for V1](../adrs/adr-003.md) — reuse the existing approval card.
- [ADR-006: Persist MCP trust and description pins in an app-managed .atelier/ store](../adrs/adr-006.md) — promote/revoke persistence.
- [ADR-007: Read-only auto-allow via the protocol resource/tool split](../adrs/adr-007.md) — description surfacing is informational, not gating.

## Deliverables
- Approval card showing full tool description + origin + trust tier.
- Promote/revoke trust from the approval flow, persisted and event-recorded.
- Transcript projection of trust events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of the first-contact → approve → promote flow **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A pending `CallMcpTool` approval renders the full (untruncated) tool description and origin server in the card.
  - [ ] Pressing the promote key marks the server trusted in the store and emits `mcp_server_trusted`.
  - [ ] A plain approve does NOT promote the server (it stays untrusted).
  - [ ] Approve/deny key handling still works with the new keys added.
- Integration tests:
  - [ ] First contact with an untrusted server prompts; after promote, a second call from that server is auto-allowed (no prompt); after revoke, it prompts again.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The approval card shows what the tool actually does; trust is explicit, remembered, and revocable.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
