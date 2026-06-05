# Technical Specification: TUI Chat Improvements

Status: Draft
Date: 2026-06-05

## Executive Summary

This specification defines the technical design for replacing the visible Event Stream with a typed Chat presentation layer. The implementation keeps `HistoryEvent` as the durable Session History model, adds typed `ChatItemView`s to app state, introduces a pure `ChatProjection` reducer, and migrates the TUI renderer from string-based event lines to typed Chat Items in phases.

The design deliberately separates durable audit data from user-facing presentation. `HistoryEvent`s remain append-only and replayable; Chat Items are derived, aggregated, severity-aware view-model objects optimized for active TUI use.

## Background / Context

The current app state stores visible activity as plain strings:

```rust
pub struct AppState {
    pub live_step: Option<LiveStepView>,
    pub pending_approval: Option<PendingApprovalView>,
    pub events: Vec<String>,
}
```

`App::record_event` appends a durable `HistoryEvent`, then pushes a display string into `state.events`. `src/tui/mod.rs` renders these strings in `render_event_stream`, with minimal styling based on string prefixes and failure words.

This creates several problems:

- one action lifecycle appears as several visible lines;
- raw runtime deltas show up as generic `Runtime stream: ...` rows;
- command output is flattened into long wrapped text;
- file edits only show file names, not focused change previews;
- recoverable denials and internal repair plumbing look like fatal errors.

The PRD for this feature is `docs/tui-chat-improvements/prd.md`. The domain glossary in `CONTEXT.md` now defines **Chat** and **Chat Item**, and marks **Event Stream** as legacy user-facing language.

## Goals

- Add a typed Chat presentation model for the TUI.
- Preserve `HistoryEvent` and Session History without schema migration.
- Derive Chat Items from active run state, pending approval state, live step state, and durable history events.
- Aggregate related action lifecycle events by `action_id`.
- Provide rich current-command summaries, focused on commands already allowed or gated by the harness.
- Provide inline diff previews for small `apply_patch` actions.
- Keep raw stdout, stderr, full action payloads, large diffs, malformed output, and artifacts accessible through typed detail references.
- Rename the visible TUI title from Event Stream to Chat.
- Implement in phases while keeping existing tests and behavior migratable.

## Non-Goals

- Do not rename or replace `HistoryEvent`.
- Do not migrate existing `.multiagent` JSONL files.
- Do not build a full interactive detail drawer in v1.
- Do not parse every command family richly in v1.
- Do not show raw runtime deltas as standalone Chat Items.
- Do not move action execution, approval handling, capability checks, or history writes into the TUI.
- Do not require a broad rename of all internal event-stream identifiers before the user-facing Chat ships.

## Requirements

### Functional Requirements

- `AppState` exposes `chat_items: Vec<ChatItemView>`.
- The TUI renders Chat from typed Chat Items when present.
- The visible panel title is `Chat`.
- `App::record_event` continues to append `HistoryEvent`s before updating Chat projection.
- A single command/action lifecycle renders as one Chat Item when linked by `action_id`.
- Recoverable denials render as warning/status items, not fatal errors.
- Run-stopping failures render as errors.
- Cargo verification/build commands get richer summaries.
- Current read-only shell/git commands get simple known-command summaries.
- High-impact approved/denied commands get approval/denial summaries.
- `apply_patch` actions can show small diff previews derived from the original action request.
- `write_file` actions show file-created summaries, not synthetic diffs.
- Raw detail access is represented with typed detail references.

### Non-Functional Requirements

- Chat projection must be deterministic from the same ordered `HistoryEvent`s.
- Projection logic must be unit-testable without Ratatui.
- Rendering must remain deterministic in Ratatui `TestBackend` tests.
- Projection must not mutate history, execute actions, or read arbitrary files.
- Projection must bound visible text and diff preview size.
- Large raw content remains artifact-backed through existing history behavior.

## Proposed Design

Introduce `src/app/chat/` as a pure view-model projection module:

```text
src/app/chat/
  mod.rs
  projection.rs
  command_summary.rs
  diff_preview.rs
```

High-level flow:

```text
App records HistoryEvent
  -> HistoryStore append_event
  -> ChatProjection.apply_history_event(event)
  -> AppState.chat_items = projection.items().to_vec()
  -> publish AppState
  -> TUI renders ChatItemView
```

Live state updates follow the same boundary:

```text
App updates live_step / pending_approval
  -> ChatProjection.apply_live_step(...)
  -> ChatProjection.apply_pending_approval(...)
  -> AppState.chat_items updated
```

The TUI receives `ChatItemView` and renders title, summary, body lines, status, severity, and detail hints. It does not infer lifecycle grouping or severity from strings.

## Architecture / Components

### App State

Add `chat_items` while keeping `events` temporarily:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppState {
    pub session_id: String,
    pub run_state: RunState,
    pub active_run_id: Option<String>,
    pub session_goal: Option<String>,
    pub config_status: ConfigStatusView,
    pub live_step: Option<LiveStepView>,
    pub pending_approval: Option<PendingApprovalView>,
    pub agents: Vec<AgentView>,
    pub chat_items: Vec<ChatItemView>,
    pub events: Vec<String>, // compatibility during migration
    pub input: String,
}
```

`events` should be removed or made debug-only after tests and rendering fully migrate.

### Chat Projection

`ChatProjection` is a reducer owned by `App`:

```rust
pub struct ChatProjection {
    items: Vec<ChatItemView>,
    index: BTreeMap<ChatLifecycleKey, usize>,
}

impl ChatProjection {
    pub fn new() -> Self;
    pub fn rebuild(events: &[HistoryEvent]) -> Self;
    pub fn apply_history_event(&mut self, event: &HistoryEvent);
    pub fn apply_live_step(&mut self, live_step: Option<&LiveStepView>);
    pub fn apply_pending_approval(&mut self, approval: Option<&PendingApprovalView>);
    pub fn items(&self) -> &[ChatItemView];
}
```

The reducer must:

- preserve first-seen ordering for lifecycle-keyed items;
- update existing items when later events share a lifecycle key;
- create one-off items for diagnostics or events that do not aggregate;
- be reconstructable from `HistoryEvent`s alone, with live/pending state applied afterward.

### TUI Renderer

Rename `render_event_stream` to `render_chat` when practical. In the first phase, it may keep internal scroll state names for a smaller diff, but the visible title must be `Chat`.

The renderer should:

- render empty state as `No chat yet.`;
- render severity/status with color plus text;
- render title and summary first;
- render bounded body lines;
- render detail labels such as `details: stdout, stderr, artifact`;
- preserve existing scroll/follow behavior.

## Data Model and Contracts

### Chat Item

V1 uses a typed envelope with renderable body lines:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatItemView {
    pub id: String,
    pub lifecycle_key: Option<ChatLifecycleKey>,
    pub kind: ChatItemKind,
    pub status: ChatItemStatus,
    pub severity: ChatSeverity,
    pub title: String,
    pub summary: Option<String>,
    pub body: Vec<ChatLineView>,
    pub details: Vec<ChatDetailRef>,
    pub source: ChatSourceRef,
    pub updated_at: String,
}
```

### Item Kinds

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatItemKind {
    UserPrompt,
    RoutingDecision,
    AgentProgress,
    ActionRequested,
    CommandResult,
    FileEdit,
    Approval,
    Diagnostic,
    AgentResult,
    RunSummary,
}
```

### Status

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatItemStatus {
    Pending,
    Running,
    WaitingApproval,
    Completed,
    Denied,
    Failed,
    Interrupted,
    Skipped,
}
```

### Severity

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatSeverity {
    Info,
    Success,
    Warning,
    Error,
}
```

### Body Lines

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatLineView {
    pub style: ChatLineStyle,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatLineStyle {
    Plain,
    Muted,
    Code,
    DiffAdd,
    DiffRemove,
    DiffContext,
    Warning,
    Error,
}
```

### Detail References

V1 defers full interactive expansion but keeps a typed detail contract:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ChatDetailRef {
    HistoryEvent {
        event_id: String,
        label: String,
    },
    Artifact {
        label: String,
        artifact_id: Option<String>,
        path: Option<String>,
        media_type: Option<String>,
    },
    Inline {
        label: String,
        content: String,
        truncated: bool,
    },
}
```

### Source References

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSourceRef {
    pub event_ids: Vec<String>,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub action_id: Option<String>,
}
```

### Lifecycle Keys

Stable lifecycle-key-derived item IDs are required:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ChatLifecycleKey {
    Prompt {
        run_id: String,
    },
    Run {
        run_id: String,
    },
    Step {
        run_id: String,
        step_id: String,
        item_kind: ChatItemKind,
    },
    Action {
        run_id: String,
        step_id: String,
        action_id: String,
    },
}
```

`ChatItemView.id` should be derived from `ChatLifecycleKey` when present, for example `chat:action:{run_id}:{step_id}:{action_id}`. One-off items without a lifecycle key should use `chat:event:{event_id}`.

Reconstruction from history must produce the same item IDs and ordering as live projection.

## APIs / Events

No new durable history event kinds are required for phase 1.

The projection maps existing history events:

| History kind | Chat behavior |
|---|---|
| `session_started` | Usually hidden or low-priority info |
| `run_started` | Creates/updates run context and user prompt item |
| `orchestrator_decision` | `routing_decision` or `run_summary` depending on decision status |
| `agent_step_started` | `agent_progress` running item |
| `runtime_stream_delta` | Not rendered directly; may update `agent_progress` |
| `action_requested` | Creates/updates action lifecycle item |
| `command_started` | Updates action lifecycle to running |
| `command_completed` | Updates command lifecycle summary |
| `file_edit_applied` | Updates file edit lifecycle summary |
| `approval_requested` | Updates action lifecycle to waiting approval |
| `action_denied` | Updates action lifecycle to denied/warning or error |
| `action_completed` | Finalizes action lifecycle |
| `artifact_written` | Adds detail ref when related; otherwise diagnostic/info |
| `agent_result` | Creates agent result item |
| `run_completed` / `run_failed` / limit events | Creates or updates run summary |
| `diagnostic` | Creates diagnostic item |

### Cross-Event Aggregation

`ChatProjection` must aggregate action-related events by `action_id`. Rich data can come from multiple events:

- command/test summaries: prefer `action_completed` payload `ActionResult.content.command/stdout/stderr/exit_code` when present;
- command milestone: use `command_started` and `command_completed`;
- file edit path and changed files: use `file_edit_applied`;
- patch preview: use earlier `action_requested.params.diff` for `apply_patch`;
- approval reason: use `approval_requested` and `action_denied` diagnostics.

Payload augmentation is allowed only when projection cannot safely derive required view data from existing related events.

## Command Summary Contract

Add a pure command summary helper under `src/app/chat/command_summary.rs`:

```rust
pub struct CommandSummaryInput {
    pub command: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub diagnostic: Option<String>,
}

pub struct CommandSummaryView {
    pub category: CommandCategory,
    pub title: String,
    pub status: ChatItemStatus,
    pub severity: ChatSeverity,
    pub body: Vec<ChatLineView>,
    pub details: Vec<ChatDetailRef>,
}
```

V1 focuses on current commands from harness policy:

- Rich Cargo summaries: `cargo test`, `cargo check`, `cargo clippy`, `cargo build`, `cargo fmt`.
- Simple known-command summaries: `pwd`, `ls`, `rg`, `grep`, `find`, `sed -n`, `cat`, `wc`, `git status`, `git diff`, `git log`, `git show`.
- Approval/denial summaries: `cargo install`, package installs, VCS mutations, `rm`, `mv`, `cp`, network commands.

Cargo parsing should use captured stdout/stderr text and extract stable visible lines such as:

- `running N tests`;
- `test result: ...`;
- `failures:`;
- `error: ...`;
- `Finished ...`;
- package/doctest target lines when useful.

V1 must not require Cargo JSON output.

## Diff Preview Contract

Add a pure diff preview helper under `src/app/chat/diff_preview.rs`:

```rust
pub struct DiffPreviewView {
    pub files: Vec<String>,
    pub added: usize,
    pub removed: usize,
    pub hunks: usize,
    pub preview_lines: Vec<ChatLineView>,
    pub truncated: bool,
}
```

Rules:

- Only `apply_patch` requests get inline diff previews in v1.
- The preview source is `action_requested.params.diff`.
- `write_file` renders as file-created path and byte count.
- Binary or invalid diffs fall back to changed files and a detail reference.
- Preview line count and byte count must be bounded.
- Large previews should set `truncated = true` and provide an artifact/history detail ref when available.

## Security and Privacy

- Chat projection must not introduce new access to secrets or files.
- Raw stdout/stderr, runtime deltas, file contents, and diffs may contain user content; keep large/raw data in existing artifacts/history rather than duplicating it into Chat.
- Inline snippets must be bounded to reduce accidental exposure and screen flooding.
- Do not expose credential values, environment values, or auth file contents beyond existing action/runtime handling.
- Approval-required actions must remain explicit and visible enough for the user to decide.
- Denied VCS and destructive commands must preserve diagnostic detail without encouraging rerun bypasses.

## Performance and Reliability

- Projection is append/update oriented and should be O(1) for lifecycle-key lookups.
- Rebuild from history is O(n) over ordered events.
- Body lines and inline details must be truncated to fixed limits.
- `chat_items` should be bounded for active display if runs become large; v1 can mirror existing event list behavior but should avoid duplicating raw payloads.
- Projection failures should not crash the app; malformed payloads should create warning diagnostics or fallback summaries.
- TUI render should not parse JSON, parse diffs, or run command recognizers.

Recommended initial bounds:

- title: 160 chars;
- summary: 240 chars;
- body lines per item: 12;
- inline detail chars: 2 KiB;
- diff preview lines: 12;
- command output lines scanned per stream: 400.

## Observability

- Keep existing `HistoryEvent` and debug logging behavior.
- Add projection-focused tests rather than runtime logs.
- When projection cannot parse an event, prefer a fallback Chat Item and optionally debug-log the projection issue.
- No analytics or telemetry are added by this feature.

## Migration and Rollout

### Phase 1: Visible Rename And Data Model

- Add `ChatItemView` contracts.
- Add `chat_items` to `AppState`.
- Initialize `chat_items` in `App::new` and test helpers.
- Render `Chat` title.
- Keep `events` fallback.

### Phase 2: Projection Reducer

- Add `ChatProjection`.
- Add it as an `App` field.
- Update `record_event` to apply history events to the projection after durable append.
- Publish `state.chat_items`.
- Add reconstruction tests from ordered history events.

### Phase 3: Action Lifecycle Aggregation

- Implement `Action` lifecycle key.
- Aggregate `action_requested`, `command_started`, `command_completed`, `approval_requested`, `action_denied`, `file_edit_applied`, and `action_completed`.
- Replace duplicate visible command/action rows with one Chat Item.

### Phase 4: Rich Summaries

- Add command summary helper for current command tiers.
- Add diff preview helper for `apply_patch`.
- Add file-created summary for `write_file`.
- Add detail refs for stdout/stderr/artifacts/history events.

### Phase 5: Test Migration And Cleanup

- Migrate app tests from `state.events` display assertions to `state.chat_items`.
- Migrate TUI tests from `Event Stream` to `Chat`.
- Keep or remove `events` depending on remaining debug value.
- Rename internal TUI scroll helpers later if useful; not required for user-facing completion.

No feature flag is required for this doc-driven implementation. The phased fallback to existing events is the compatibility mechanism.

## Testing Strategy

### Unit Tests

- `ChatProjection` creates user prompt, routing, agent result, diagnostic, and run summary items.
- Replaying the same ordered `HistoryEvent`s produces stable IDs and ordering.
- Multiple action lifecycle events aggregate into one item.
- Recoverable denials produce warning severity.
- Run-stopping failures produce error severity.
- Runtime stream deltas do not create generic Chat Items.
- Malformed payloads produce fallback diagnostics instead of panics.

### Command Summary Tests

- `pwd` produces a simple known-command summary.
- `cargo test ...` with passing output extracts `test result: ok`.
- `cargo test ...` with failure output extracts failure/error lines.
- `cargo check`, `cargo clippy`, `cargo build`, and `cargo fmt` produce rich Cargo summaries.
- Unknown commands use fallback command/status/exit-code summaries.
- Approval-required `cargo install` or VCS mutations show approval/denial summaries without rich stdout parsing.

### Diff Preview Tests

- Small `apply_patch` diff produces files, added/removed counts, hunk count, and preview lines.
- Large diff truncates preview.
- Invalid/binary diff falls back without panicking.
- `write_file` renders file-created path and byte count without diff preview.

### App Integration Tests

- `App::record_event` appends history and updates `chat_items`.
- Fake runtime command action results in one command Chat Item.
- Approval denial updates the same action lifecycle item.
- File write action produces a file-created Chat Item.
- Parse repair plumbing is hidden unless user-visible or failing.

### TUI Render Tests

- Empty state shows `Chat` and `No chat yet.`
- Typed items render title, summary, body, severity/status text, and detail hints.
- Roster hidden mode gives Chat full width.
- Scroll/follow behavior still follows latest Chat Item.
- Existing pending approval display is represented through Chat or remains visually compatible during migration.

## Alternatives Considered

### Continue Rendering Strings

Rejected. It keeps product semantics mixed into display strings and makes aggregation brittle.

### Rename History Events To Chat Events

Rejected. It creates migration churn and erases the useful distinction between durable audit history and presentation.

### Put Projection Logic In Ratatui Renderer

Rejected. Rendering would become hard to test and would need to parse history/action payloads on every draw.

### Add Fully Interactive Detail Drawer In V1

Rejected for scope. Typed detail refs provide the data contract first; interactive expansion can build on it later.

### Rich Parsers For Many Toolchains

Rejected for v1. The implementation should focus on commands already allowed or gated by current harness policy, with deep parsing only for Cargo verification/build commands.

## Risks and Mitigations

- Risk: Projection and old `events` diverge during migration.
  Mitigation: prefer projection in render when non-empty and migrate tests phase by phase.

- Risk: Cross-event aggregation misses data when events arrive in unexpected order.
  Mitigation: reducer must tolerate partial items and update when later data arrives.

- Risk: Command output parsing is brittle.
  Mitigation: parse only stable visible Cargo lines and always provide fallback summaries.

- Risk: Diff previews expose too much user content.
  Mitigation: fixed preview limits and artifact/detail refs for large content.

- Risk: Chat hides useful debugging evidence.
  Mitigation: keep `HistoryEvent`, debug logs, artifacts, and detail refs.

- Risk: Adding `chat_items` increases memory use.
  Mitigation: avoid raw payload duplication and bound inline body/detail content.

## Open Questions

- Should `events` be removed entirely after migration or retained as debug-only state?
- What exact keybindings should open future interactive detail expansion?
- Should future resume load `chat_items` by replaying all history events or use a cached projection snapshot?
- Should future stream-mode coalesced runtime events update one `agent_progress` item or multiple stream-specific progress items?

## Acceptance Criteria

- `docs/tui-chat-improvements/prd.md` requirements are represented in code-facing contracts.
- `AppState` includes typed `chat_items`.
- `ChatProjection` can rebuild Chat Items from ordered `HistoryEvent`s.
- TUI renders a visible `Chat` surface.
- Action lifecycle events aggregate into one visible Chat Item by action id.
- Command summaries cover the current three-tier command scope.
- `apply_patch` previews show bounded inline diff metadata.
- `write_file` shows file-created summaries.
- Raw details are represented through typed detail refs.
- Existing durable history remains valid and unchanged.
- Unit, integration, and TUI render tests cover the projection and rendering boundaries.
