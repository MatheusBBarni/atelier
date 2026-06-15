# Task Memory: task_10.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Phase 2 first-approval explainer: a single muted line on the FIRST approval a user ever hits,
show-once across sessions via a persisted latch. Additive to approval render; no semantics change.

## Important Decisions
- ADR-004: latch = `.atelier/ui_state.json` (root-level JSON `{first_approval_explainer_shown}`),
  not per-session (fresh dir each launch) and not ephemeral UI state. Survives `clean_sessions`.
- `AppState.show_first_approval_explainer: bool` (`#[serde(default)]`) is the render channel.
  App decides once at approval-creation time; render stays pure (no FS I/O).
- Both approval surfaces gate on the flag: projected `apply_pending_approval` body (live) and
  the `:2492` fallback (dead in prod — sync_chat_items always prepends welcome — but unit-tested).
- Explainer styled with `ChatLineView::muted` / `theme.text_muted` (no inline `Color::`).

## Learnings
- AppState has NO Default; full literals must add the field. Sites: app/mod.rs:751 + tui tests
  4664/4706/4733/4954/5047/7111 + helpers state_with_input(7217)/state_with_agent_roster(7326)/
  state_with_queue(8321). Many other tests spread off the helpers (no edit needed).
- ChatLineStyle::Muted -> theme.text_muted (tui mod.rs ~:3281).

## Files / Surfaces
- src/history/mod.rs — latch read/write + PersistentUiState struct.
- src/app/mod.rs — AppState field; gate-once in approval block (~:3826); pass flag in sync_chat_items.
- src/app/chat/projection.rs — apply_pending_approval gains `show_explainer` param.
- src/tui/mod.rs — fallback approval render (~:2492) appends explainer when flag set.

## Errors / Corrections
- Rendered-text fallback test failed at width 100: the long explainer line wraps, breaking
  `text.contains(full_string)`. Fix = render the positive case at width 200 (no wrap). Projection
  and app-integration tests assert on body `ChatLineView.text` directly, so they are wrap-immune.

## Ready for Next Run
- task_10 done; gate green (fmt/clippy --all-targets/`cargo test --locked` = 735 lib + suites, 0 fail).
- Whole help-modal-tabs PRD (task_01–10, MVP + Phase 2) is now complete.
