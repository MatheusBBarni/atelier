# Plan: TUI Improvements Toward Codex/Claude Code Style

Status: Draft
Date: 2026-06-03

## Summary

The current TUI is functional: it has an Agent Roster, Chat, Input
Composer, help modal, scroll behavior, and basic key handling. To feel closer to
modern coding CLIs such as Codex CLI or Claude Code, it needs to become more
transcript-centric, keyboard-fluent, status-rich, and action-aware.

This plan keeps the existing Ratatui/Crossterm stack and app-state boundary. The
TUI should remain a renderer and input surface; it should not execute runtimes,
write history, or bypass app policy.

## Current TUI baseline

Relevant current behavior in `src/tui/mod.rs`:

- Alternate-screen Ratatui app.
- Left roster plus right event stream.
- Bottom input composer.
- Roster visibility toggle.
- Help modal.
- PageUp/PageDown/Home/End event scrolling.
- Arrow-key input cursor movement.
- Pending approval display.
- Render tests using Ratatui `TestBackend`.

Limitations:

- Events are plain strings, so prompts, plans, commands, diffs, approvals, and
  results cannot be styled or navigated as distinct items.
- The composer is basic and has no history/search/command mode.
- There is no command palette or slash-command registry.
- There is no live streaming message view.
- The roster takes a fixed percentage and does not adapt deeply to small
  terminals.
- There is no dedicated status/footer bar showing current shortcuts.

## Design direction

Use a persistent multi-panel layout with a transcript-first center:

```text
+ atelier /path/to/repo ------------------------- running: fixer ----+
| Agents / Plan       | Transcript                                      |
| > orchestrator idle | You: add stream mode                           |
|   explorer    done  | Orchestrator plan ...                          |
|   fixer       run   | Fixer streaming output ...                     |
|                     |                                                |
| Context             | Action / Detail drawer when focused             |
| goal, cwd, runtime  | command output, diff preview, approval details  |
+-----------------------------------------------------------------------+
| > composer input...                                                    |
+ [Tab]focus [?]help [/]search [:]command [Esc Esc]interrupt -----------+
```

Responsive behavior:

- `>= 120 cols`: roster/context left, transcript right, composer/footer bottom.
- `80-119 cols`: compact roster left, transcript right.
- `< 80 cols` or `< 24 rows`: show a resize message or collapse to
  transcript-only mode.
- Roster can be toggled, but panel positions should remain predictable.

## Codex-inspired behavior targets

Current Codex CLI behavior worth matching where it fits this harness:

- Syntax-highlighted markdown, code blocks, and diffs in transcript items.
- A model command that makes model selection visible and explicit.
- Prompt draft history and search.
- Follow-up prompts can be queued while a run is active.
- Copy and clear commands operate on visible transcript content.
- Resume/recent-session navigation is first-class.
- Theme selection is available without editing config by hand.

These are TUI/app behaviors, not reasons for the TUI to execute actions
directly.

## Information architecture

### Header

Show compact global state:

- app name
- current working directory basename
- run state
- active agent
- runtime/model for active agent
- pending approval indicator

### Agent/Plan panel

Replace the roster-only panel with a richer left rail:

- agent status, runtime/model, availability
- current run plan from Orchestrator
- active session goal when implemented
- compact limits/counters when relevant

### Transcript

Move from `Vec<String>` rendering to typed transcript items:

```rust
pub enum TranscriptItemView {
    UserPrompt { text: String },
    RunPlan { steps: Vec<String> },
    AgentStarted { agent: String, step_id: String },
    Stream { agent: String, content: String, final_delta: bool },
    ActionRequested { agent: String, kind: String, summary: String },
    Command { command: String, status: String },
    FileEdit { path: String, status: String },
    Approval { summary: String, diagnostic: Option<String> },
    AgentResult { agent: String, summary: String, findings: Vec<String> },
    Diagnostic { severity: String, message: String },
}
```

The app can continue storing history as JSONL. The TUI should receive a typed
view model derived from app state and history events.

### Detail drawer

Add a focused detail area for:

- full command output
- patch/diff preview
- approval details
- parse-error artifact path
- selected transcript item payload

This can start as an expanded modal before becoming a permanent drawer.

### Composer

Improve the composer in phases:

- Multi-line wrapping with stable cursor behavior already exists; keep it.
- Add prompt history with Up/Down only when the composer is at first/last line,
  or use `Ctrl-P`/`Ctrl-N` to avoid conflict with cursor movement.
- Add prompt history search, preferably `Ctrl-R` if it does not conflict with
  terminal expectations in this TUI.
- Allow submitting a follow-up while a run is active by queuing it visibly
  instead of rejecting input.
- Add `Esc` to leave command/search modes, not to quit immediately.
- Add `Alt-Enter` or `Ctrl-J` for newline if Enter remains submit.
- Add paste-safe insertion and large paste handling.
- Show pending approval affordance in the composer title.

## Interaction model

Use layered keyboard behavior:

- Always visible footer:
  - `[Tab]focus [?]help [/]search [:]command [Esc Esc]interrupt`
- Direct keys:
  - `Tab` / `Shift-Tab`: cycle focus
  - `?`: help
  - `/`: transcript search
  - `:`: command palette
  - `Esc Esc`: request soft interrupt for an active run
  - `Ctrl-C`: hard quit or terminal interrupt fallback
  - `q`: quit only when idle or from help/detail contexts
  - `Enter`: submit composer or confirm focused item
  - `PageUp/PageDown`: scroll active scrollable panel
  - `Home/End` or `g/G`: jump top/bottom
- Approval mode:
  - `y`: approve
  - `n`: deny
  - `d`: open detail/diff
  - `Esc`: return focus without answering

Avoid rebinding terminal-level signals beyond current interrupt handling.

## Slash commands and command palette

Start with a TUI-local command registry. Commands should dispatch `AppEvent`s,
not call app internals directly.

Initial commands:

- `/help`: toggle help.
- `/doctor`: append current diagnostic summary to transcript.
- `/agents`: focus/toggle agent panel.
- `/clear`: clear visible transcript for the session view only, or ask before
  deleting history.
- `/interrupt`: interrupt active run.
- `/sessions`: show project-local recent sessions.
- `/config`: show config source summary.
- `/copy`: copy selected or last transcript item when clipboard support is
  available.
- `/model`: show or change the active model when runtime support exists.
- `/review`: start a review-oriented run preset when configured.
- `/resume`: open recent-session navigation.
- `/theme`: select a configured TUI theme.

Future commands after related features:

- `/preset`
- `/goal`
- `/export`

The `:` palette can expose the same commands with fuzzy matching.

## Visual system

Use restrained terminal colors:

- Cyan/blue: active focus and user prompts.
- Green: completed/success.
- Yellow: waiting/approval/warnings.
- Red: failed/denied/interrupted.
- Gray: metadata and inactive chrome.
- White/default: main transcript text.

Rules:

- Use color plus text labels; do not rely on color alone.
- Keep borders dim except for the focused panel.
- Keep the transcript readable on 16-color terminals.
- Avoid heavy nested boxes; use spacing, prefixes, and focused borders.
- Keep line wrapping deterministic in tests.

## Streaming integration

This TUI plan depends on `docs/stream-mode.md` for live output.

TUI requirements for streaming:

- A live stream block updates in place.
- Manual scroll disables follow mode.
- Jump-to-bottom re-enables follow mode.
- Long streams wrap without shifting the composer.
- Final output is promoted into the transcript as a durable item.

## Approvals and action previews

Approvals should feel like coding CLI approvals:

- Show the agent, action kind, and reason.
- For commands, show the exact command and working directory.
- For edits, show file path and patch/diff preview when available.
- For denied actions, show the policy reason.
- Require explicit answer; empty Enter should not approve by accident.

Implementation:

- Extend `PendingApprovalView` with optional `kind`, `command`, `path`, and
  preview fields.
- Keep the actual approve/deny handling in `App::resolve_pending_approval`.

## Responsive testing matrix

Every layout change should be covered at:

- 80x24: minimum normal terminal.
- 100x24: common laptop split.
- 120x40: comfortable default.
- 200x60: wide monitor.

Assertions:

- No panic.
- Header, transcript, composer, and footer are present or intentionally
  collapsed.
- Text does not overlap.
- Focus state is visible.
- Pending approval remains visible.

## Implementation phases

### Phase 1: View model cleanup

- Introduce typed transcript view items.
- Convert current string events into typed render items at the app boundary.
- Keep history storage unchanged.
- Preserve existing TUI tests.

### Phase 2: Header and footer

- Add header with cwd/run/agent status.
- Add contextual footer with 3-5 active shortcuts.
- Add focus state and `Tab` focus cycling.

### Phase 3: Composer and commands

- Add command registry for slash commands.
- Add prompt history.
- Add search mode.
- Improve approval input handling.

### Phase 4: Detail views

- Add detail modal/drawer for selected transcript items.
- Add command output and diff previews.
- Add parse-error artifact references.

### Phase 5: Streaming polish

- Render live stream view from stream-mode state.
- Add follow/unfollow indicators.
- Add active step elapsed time.

### Phase 6: Responsive layout

- Add breakpoint layout functions.
- Collapse roster/context panel on narrow terminals.
- Add minimum-size message.

## Acceptance criteria

- `cargo test` covers the new TUI render states.
- The TUI has a persistent footer showing current shortcuts.
- The transcript is typed enough to render prompts, actions, approvals, and
  results differently.
- Follow-up input entered during an active run is either queued visibly or
  rejected with a clear transcript event.
- Approval prompts cannot be approved accidentally by pressing Enter on empty
  input.
- The layout remains coherent at 80x24, 120x40, and 200x60.
- The TUI still does not execute runtimes, actions, or history writes directly.
