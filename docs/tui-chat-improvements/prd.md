# Product Requirements: TUI Chat Improvements

Status: Draft
Date: 2026-06-05

## Summary

Replace the current visible Event Stream with a user-facing Chat surface that presents useful, curated Chat Items instead of raw event/log lines. The Chat should feel closer to modern agent CLIs: concise status updates, rich command/test summaries, inspectable file edits, severity-aware diagnostics, and final outcomes that are easy to scan.

This feature changes the TUI presentation model, not the durable history model. `Session History` and `HistoryEvent` remain the source of durable audit data. Chat is a derived presentation layer built from active run state, live progress, and Session History.

## Problem

The current TUI renders event display strings directly. That produces noisy output:

- runtime stream entries show generic labels like `Runtime stream: stdout`;
- one harness action can produce several visible lines for request, denial, completion, and raw command text;
- missing action parameters and recoverable denials look like terminal failures;
- command output is flattened into long wrapped lines;
- file edits are not inspectable as focused `+N -N` changes;
- internal repair events are shown even when they do not help the user decide anything.

The result is technically accurate but not useful enough while a run is active. Users need to understand what happened, what is happening now, what needs attention, and what changed, without reading raw logs.

## Goals

- Rename the user-facing surface from Event Stream to Chat.
- Render curated Chat Items instead of raw event lines.
- Collapse related action lifecycle events into one visible item.
- Show command/test runs as rich summaries when recognizable.
- Show file edits with path, `+N -N`, and small inline diff previews.
- Use severity-aware diagnostics so recoverable denials do not look like fatal failures.
- Keep raw stdout, stderr, full command output, large diffs, malformed runtime output, and oversized payloads available through details, artifacts, Session History, or debug output.
- Keep Chat derived from app/view-model state, not from TUI-only string parsing.
- Preserve the durable `HistoryEvent` model and avoid history schema migration for this feature.
- Implement the feature in phases so existing app/runtime behavior remains testable during migration.

## Non-Goals

- Do not rename or replace `Session History` as the durable record.
- Do not migrate existing history schemas from events to chat.
- Do not expose a raw/event mode in the main TUI for v1.
- Do not show every runtime token, stdout chunk, or stderr chunk as its own Chat Item.
- Do not move harness action execution, capability enforcement, or approvals into the TUI.
- Do not make stream mode, command parsing, or diff rendering provider-specific.
- Do not require a large internal rename of every function currently named event stream before the user-facing Chat can ship.

## Domain Language

- **Chat**: the primary TUI surface that presents prompts, routing decisions, agent activity, harness actions, command results, file edits, diagnostics, and results.
- **Chat Item**: a curated unit rendered in Chat.
- **Session History**: the durable project-local record of sessions, runs, plans, outputs, commands, and verification evidence.
- **HistoryEvent**: the internal durable event model used to persist Session History.

Event Stream is legacy user-facing language. After this feature lands, visible UI and documentation should say Chat. Internal event/history terminology may migrate incrementally where it improves clarity.

## User Stories

1. As a user, I want the main activity surface to read like a useful agent chat, so I can quickly understand a run without parsing raw logs.
2. As a user, I want one command to appear as one Chat Item that updates from running to completed or failed, so repeated lifecycle lines do not clutter the screen.
3. As a user, I want test commands to show recognizable test summaries, so I can tell what passed or failed at a glance.
4. As a user, I want file edits to show paths, line counts, and small diff previews, so I can inspect changes without opening artifacts for every edit.
5. As a user, I want recoverable denials and missing runtime action fields to appear as warnings unless they stop the run, so I can distinguish normal repair from failure.
6. As a user, I want raw output to remain accessible when needed, so concise Chat summaries do not hide debugging evidence.
7. As a maintainer, I want Chat Items to be typed view-model data, so product behavior is testable outside terminal rendering.
8. As a maintainer, I want Chat to be derived from active app state and Session History, so durable history remains separate from presentation.
9. As a maintainer, I want a phased implementation, so existing `state.events` tests and runtime behavior can migrate without a risky rewrite.

## Chat Item Types

The first useful version should support:

- `user_prompt`: the prompt submitted through the Input Composer.
- `routing_decision`: the Orchestrator's selected next agent, reason, and plan context.
- `agent_progress`: compact active-agent progress, updated periodically when useful.
- `action_requested`: a visible pending harness action when it matters to the user.
- `command_result`: command/test execution status, summary, exit code, and collapsed details.
- `file_edit`: changed path(s), `+N -N`, and inline diff preview when small.
- `approval`: action approval prompt, decision, denial reason, and relevant preview.
- `diagnostic`: warning/error/info items with clear severity.
- `agent_result`: the structured outcome from a Specialized Agent.
- `run_summary`: final run status, key changes, verification, blockers, and next steps.

Do not expose `runtime_stream_delta` as a generic visible Chat Item. Runtime deltas may feed `agent_progress` when they add user value.

## Presentation Requirements

### Curated By Default

Chat must prioritize high-signal summaries over raw data. A Chat Item should answer:

- what happened;
- who or what did it;
- whether it is running, completed, failed, denied, or waiting;
- what changed or needs attention;
- where to inspect raw details.

### Lifecycle Aggregation

Related lifecycle events should collapse into one Chat Item when they describe one visible lifecycle. A command action should not produce separate durable-looking Chat rows for:

- action requested;
- command started;
- command completed;
- action completed;
- action denied.

Instead, Chat should update or replace one item keyed by run, step, and action id.

### Command And Test Summaries

Recognized commands should render rich summaries. Test commands should show:

- command name and target;
- status;
- exit code when available;
- concise result lines, such as `test result: ok...`;
- collapsed stdout/stderr details.

Unknown commands should fall back to command, status, exit code, and concise diagnostic.

### File Edit Previews

File edit Chat Items should show:

- path or path summary;
- `+N -N` counts;
- small inline diff preview when focused and readable;
- artifact/detail reference for large diffs or many files.

Large diffs must not flood Chat.

### Severity-Aware Diagnostics

Diagnostics should use severity based on user impact:

- `info`: normal progress or hidden internal repair that is worth explaining.
- `warning`: recoverable denial, missing action field, retry, or degraded behavior.
- `error`: run-stopping failure, required user intervention, or unrecoverable runtime/action failure.

Recoverable policy denials and parser repair steps should not look fatal unless the run stops or waits for the user.

### Internal Repair Visibility

Internal repair and retry events should be hidden unless they explain user-visible behavior. Show them when they:

- caused a retry that delays the run;
- produced an artifact the user may need;
- changed the final outcome;
- failed and stopped the run.

### Live Progress

Live runtime output should not stream raw tokens into Chat. For v1, active runtime output should appear as compact `agent_progress` updates such as working, inspecting, running, waiting for approval, or summarizing. Meaningful milestones become durable Chat Items.

## Data And Architecture Requirements

Chat Items should be built in the app/view-model layer, not in Ratatui rendering code.

The target state shape should move toward:

```rust
pub struct AppState {
    // existing fields...
    pub chat_items: Vec<ChatItemView>,
}
```

The renderer should receive typed `ChatItemView`s and handle layout, wrapping, color, focus, and expansion only. It should not infer product semantics from strings.

`HistoryEvent` remains durable. Chat projection should derive from:

- active run state;
- pending approval state;
- live step state;
- newly recorded `HistoryEvent`s;
- reconstructed Session History when loading or resuming.

`record_event(..., display)` should migrate toward appending durable history first and updating a typed Chat projection second. The display string should not be the primary presentation contract.

## Implementation Phases

### Phase 1: Rename Visible Surface And Introduce Typed Chat Items

- Change user-facing TUI title from Event Stream to Chat.
- Add `ChatItemView` and `ChatItemKind` in the app/view-model layer.
- Add `AppState.chat_items` while keeping `AppState.events` temporarily for compatibility.
- Render Chat from typed items when present, falling back to existing events during migration.
- Add render tests for the Chat title and basic item kinds.

### Phase 2: Project Existing Events Into Chat

- Update `record_event` call sites or helper functions to create typed Chat Items.
- Preserve `HistoryEvent` writes unchanged.
- Map current prompt, routing, agent result, run summary, diagnostic, approval, and limit events into Chat Items.
- Keep raw history/debug output available outside the main Chat.
- Migrate tests that assert `state.events` strings toward `chat_items` expectations.

### Phase 3: Aggregate Action Lifecycles

- Key action-related Chat Items by run id, step id, and action id.
- Merge action requested, command started, command completed, action denied, action completed, approval requested, and file edit events into one visible lifecycle where appropriate.
- Represent status transitions explicitly: requested, running, waiting approval, completed, denied, failed.
- Ensure denied-but-recoverable actions render as warnings, not fatal errors.

### Phase 4: Rich Command/Test And File Edit Rendering

- Add command summary extraction with test-command recognition.
- Capture command output snippets suitable for Chat body text.
- Add file edit summary metadata: changed files, added/deleted counts, and small diff previews.
- Store or link large outputs/diffs through artifacts/details.
- Add tests for cargo test summaries, unknown command fallback, small diff previews, and large diff collapse.

### Phase 5: Live Progress And Detail Expansion

- Feed live runtime progress into compact `agent_progress` items.
- Avoid rendering generic `runtime_stream_delta` items.
- Add a detail view or expansion model for raw stdout/stderr, full command output, patch previews, and artifacts.
- Keep manual scroll/follow behavior compatible with the stream-mode requirements.

## Acceptance Criteria

- No user-facing TUI title or primary docs call the surface Event Stream.
- The TUI shows a Chat surface with typed Chat Items.
- A command lifecycle appears as one visible Chat Item instead of several duplicated lines.
- Recognized test commands show a rich result summary.
- File edits can show `+N -N` and a small diff preview.
- Recoverable denied actions render as warnings/status items unless they stop the run.
- Internal repair events are hidden unless user-visible.
- Raw output remains available through details, artifacts, Session History, or debug output.
- `HistoryEvent` remains the durable model and does not require schema migration.
- Tests cover app-level Chat projection and TUI rendering separately.

## Open Questions For Techspec

- Exact `ChatItemView` struct fields and serialization requirements.
- Whether `chat_items` should persist in memory only or have a cache for resume.
- How much diff parsing should live in action recording versus a dedicated projection module.
- The exact command recognition rules for `cargo test`, `cargo check`, `npm test`, and other toolchains.
- How detail expansion is focused and navigated in Ratatui.
- Whether older `state.events` should be removed after migration or retained as debug-only state.
