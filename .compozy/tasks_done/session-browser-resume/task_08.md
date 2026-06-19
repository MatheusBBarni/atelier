---
status: completed
title: "Transcript preview pane (off-thread fold load)"
type: frontend
complexity: high
dependencies:
  - task_06
  - task_07
---

# Task 08: Transcript preview pane (off-thread fold load)

## Overview
Add the read-only transcript preview to the browser: selecting a session loads its sanitized, history-only projection (task_06) off-thread via a watch side-channel and renders it as a scrollable pane, completing the Phase 1 browse + preview MVP. The preview is the user's verification view before any resume.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a preview mode to the browser: selecting a session (`Enter`) loads its preview and switches to a preview view; `Esc`/back returns to the list.
- MUST load the preview OFF-THREAD via a watch side-channel (the preview builder from task_06), never blocking the render loop, showing a loading state until it arrives.
- MUST render the preview as a scrollable transcript (reusing the chat scroll conventions: PageUp/PageDown/Home/End) with sanitized text.
- MUST keep the preview strictly read-only — selecting/previewing MUST NOT mutate live `App` state.
</requirements>

## Subtasks
- [x] 8.1 Add the list→preview transition (`Enter` selects, back returns) and preview state (scroll offset).
- [x] 8.2 Add the off-thread preview loader + watch side-channel, with a loading placeholder.
- [x] 8.3 Render the sanitized preview transcript with scroll support.
- [x] 8.4 Add unit/integration tests for the transition, off-thread load, and read-only guarantee.

## Implementation Details
Extend the browser state/commands from task_07 in `src/tui/mod.rs` with a `Preview` mode and scroll offset; mirror the `spawn_file_index_refresh`/watch pattern for a `watch::Sender<Option<SessionPreview>>` carrying the task_06 output. Reuse the chat scroll commands (`ScrollEvents`/`EventScrollCommand`, around `:1708`) for the preview pane. Render near the existing modal renderers. See TechSpec "System Architecture" (data flow) and ADR-001/004.

### Relevant Files
- `src/tui/mod.rs` — browser state/commands (task_07), scroll (`:1708`), `spawn_file_index_refresh` (`:947`), `render` (`:2522`).
- `src/app/chat/` — the preview builder (task_06).

### Dependent Files
- `src/tui/mod.rs` — task_11 adds the `Resume` action from the preview/list (dispatching `AppEvent::ResumeSession`).

### Related ADRs
- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — preview is in V1 as the confirmation view.
- [ADR-004: Resume safety model](adrs/adr-004.md) — sanitized, read-only rendering of untrusted transcript bytes.

## Deliverables
- A scrollable, sanitized, off-thread-loaded transcript preview pane in the browser.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: select → off-thread preview load → scroll → back **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `Enter` on a selected row switches the browser to preview mode and requests a load; `Esc` returns to the list. — `enter_opens_preview_and_esc_returns_to_list`
  - [x] Before the preview arrives, a loading placeholder renders; after the watch channel updates, the transcript renders. — `preview_shows_loading_placeholder_then_transcript`
  - [x] PageUp/PageDown/Home/End adjust the preview scroll offset within bounds. — `preview_scroll_stays_within_bounds`
  - [x] Entering preview does not change live `AppState.chat_items` or run state. — `entering_preview_does_not_mutate_app_state`
- Integration tests:
  - [x] Open browser → select a recorded session → preview loads off-thread and matches the on-disk fold → scroll → back to list. — `preview_matches_the_on_disk_fold` (+ the transition/scroll/back tests above; off-thread plumbing via `sync_session_preview`/`spawn_session_preview_load`).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Phase 1 MVP complete: a user can browse and read any past session's full transcript without resuming, without UI stalls.

## As-built notes
- `BrowserMode::Preview` + `SessionBrowserState { preview: Option<SessionPreview>, preview_session_id, preview_scroll }`. `SessionBrowserCommand` gains `OpenPreview` (Enter), `Back` (Esc), `ScrollPreview(EventScrollCommand)`. `session_browser_key_command` is now mode-aware (List: Enter/nav/filter/close; Preview: Esc-back + chat-scroll keys PageUp/PageDown/Up/Down/Home/End).
- Off-thread load mirrors task_07: `run_loop` owns a `watch<Option<SessionPreview>>`, spawns `spawn_session_preview_load` (→ `spawn_blocking(build_session_preview)`, task_06) when `preview_session_id` changes, and `sync_session_preview` adopts it. Until it lands, `preview` is `None` → a "Loading transcript…" placeholder renders.
- `render_session_preview` draws the sanitized transcript (titles via `severity_title_style`, body via the existing `chat_body_line` — all theme tokens, colors guard still green) with `Paragraph::scroll`. `preview_total_lines` keeps `apply_preview_scroll`'s clamping in lockstep with the render.
- **Read-only:** `apply_session_browser_command` takes only `ui_state`; the preview builder owns a throwaway projection. A test confirms `OpenPreview` leaves `AppState.chat_items`/`run_state` untouched.
