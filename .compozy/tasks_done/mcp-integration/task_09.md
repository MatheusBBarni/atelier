---
status: completed
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
- [x] 9.1 Extend the pending-approval view with tool description, origin server, and trust tier.
- [x] 9.2 Render those fields in the approval card.
- [x] 9.3 Add key handling to promote server trust from the approval flow (the `t` key reuses `ApproveAndTrust`).
- [x] 9.4 Persist promote via the trust store and emit events (revoke via `/trust` + `revoke_mcp_server`).
- [x] 9.5 Project `mcp_server_trusted`/`mcp_server_revoked` into the transcript.

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
  - [x] A pending `CallMcpTool` approval renders the full (untruncated) tool description, origin server, and trust tier in the card. (`projection::tests::mcp_approval_card_shows_full_description_and_origin`)
  - [x] The promote key (`t`) resolves to `ApproveAndTrust`, and a plain approve (`y`) resolves to `ApproveOnce` — so a plain approve does NOT promote. (`tui::tests::mcp_approval_offers_promote_and_keeps_approve_deny`)
  - [x] Promote persists trust and emits `mcp_server_trusted`. (`app::tests::promote_mcp_server_persists_file_and_records_event`, task_04 — the exact call `resolve_pending_approval` makes for an MCP `ApproveAndTrust`.)
  - [x] Approve/deny key handling still works with the new keys added. (same TUI test: `y`→approve, `n`→deny.)
  - [x] Trust promote/revoke project into the transcript. (`projection::tests::mcp_trust_events_project_into_transcript`)
- Integration tests:
  - [x] First contact with an untrusted server prompts; after promote, a call is auto-allowed (no prompt); after revoke, it prompts again. (`tests/mcp_actions.rs::first_contact_prompts_then_promote_allows_then_revoke_prompts`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Reuses the existing `ApproveAndTrust` resolution.** The `t` key already maps to `ApproveAndTrust`; the gating that *offered* it was `trust_target.is_some()`. A new `PendingApprovalView::offers_trust()` extends that to also offer it when `mcp_server` is set (MCP risk notes carry no session `trust_target`). `resolve_pending_approval` then promotes the origin server in the durable trust store (via `promote_mcp_server`, ADR-006) when `grants_trust()` and the action is `CallMcpTool` — a plain `ApproveOnce` never promotes.
- **Card vs. modal.** The full untruncated description + origin + tier render in the chat **Approval card** (`apply_pending_approval`); the TUI modal footer gains the `t = approve & trust` hint. Description fields populate in `build_pending_approval_view` from `context.mcp` (the snapshot wired in task_07).
- **Revoke** is driven by `App::revoke_mcp_server` (task_04) / `/trust` rather than a dedicated approval-card key (there is no card shown for an already-trusted, auto-allowed call); both promote and revoke emit events that now project into the transcript (`apply_mcp_trust_event`).
- **App-level resolve→promote test deferred to composition.** The fake runtime cannot emit a `CallMcpTool` action, so a full `submit_prompt`→pending-MCP-approval→resolve test isn't feasible without new runtime scaffolding. The behavior is covered by composition: `t`→`ApproveAndTrust` (TUI test) + `promote_mcp_server` persists+emits (task_04 test) + the promote→gating transitions (integration test).

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- Projection (card + trust events) + TUI resolution + 3 integration tests: pass.
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1337 passed, 4 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- The approval card shows what the tool actually does; trust is explicit, remembered, and revocable.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
