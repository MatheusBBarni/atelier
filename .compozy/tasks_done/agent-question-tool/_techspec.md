# Technical Specification: Clarification Select UI

Status: Draft
Date: 2026-06-07
Source PRD: `.compozy/tasks/agent-question-tool/_prd.md`

## Executive Summary

Clarification Select UI turns the existing free-text `WaitingForUser` clarification flow into structured orchestrator-owned state. The Orchestrator returns a Clarifying Question with 2-4 structured options and an optional recommended option id. The app exposes that state through `AppState.pending_clarification`, the TUI renders it as a select-style Input Composer with an always-visible custom text field, and the app records the submitted answer through a dedicated clarification answer event.

The primary trade-off is accepting small contract changes across the orchestrator schema, runtime prompt examples, app state, Chat projection, and TUI instead of inferring options from free text. This is more explicit than the current path, but keeps V1 scoped to orchestrator Clarifying Questions and avoids a broader any-agent question tool.

## System Architecture

### Component Overview

- **Orchestrator Contract**  
  Owns the structured question payload on `OrchestratorDecision` for `status = waiting_for_user`.

- **Runtime Prompt Contracts**  
  Codex, Claude, Cursor, Z.ai guidance, and fake runtime examples describe the expanded orchestrator decision shape so runtimes return options consistently.

- **App Clarification State**  
  Stores the pending question as public `PendingClarificationView` in `AppState` and keeps the existing private `PendingClarification` resume context.

- **App Answer Path**  
  Accepts `AppEvent::ClarificationAnswered`, records answer metadata, appends the answer to the run prompt, clears pending state, and resumes the run.

- **TUI Composer Mode**  
  Detects `state.pending_clarification`, routes Up/Down to option selection, Enter to answer submission, and text editing to the custom answer field.

- **Chat Projection**  
  Adds clarification-specific Chat semantics so pending questions do not render as approvals.

- **Session History**  
  Records compact `clarification_requested` and `clarification_answered` events with question id, selected option metadata, answer source, and answer text.

## Implementation Design

### Core Interfaces

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClarificationOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingClarificationView {
    pub run_id: String,
    pub question_id: String,
    pub question: String,
    pub options: Vec<ClarificationOption>,
    pub recommended_option_id: Option<String>,
}
```

### Data Models

#### `OrchestratorDecision`

Add fields:

- `clarifying_options: Vec<ClarificationOption>`
- `recommended_option_id: Option<String>`

Validation rules for `DecisionStatus::WaitingForUser`:

- `clarifying_question` is non-empty.
- `clarifying_options.len()` is between 2 and 4.
- Every option id and label is non-empty.
- Option ids are unique.
- `recommended_option_id`, when present, matches an option id.
- Terminal waiting decisions still cannot include `next_agent` or `next_step`.

#### `AppState`

Add:

- `pending_clarification: Option<PendingClarificationView>`

Keep private `PendingClarification` for the paused `RunDriveContext`.

#### `AppEvent`

Add:

- `ClarificationAnswered(ClarificationAnswer)`

`ClarificationAnswer` fields:

- `question_id`
- `answer`
- `selected_option_id: Option<String>`
- `selected_option_label: Option<String>`
- `answer_source: recommended | custom`

#### Chat

Add:

- `ChatItemKind::Clarification`
- `ChatItemStatus::WaitingForUser`

Use `Clarification` for pending question and answered state. Do not reuse `Approval`.

#### History Events

Use explicit lifecycle events:

- `clarification_requested`
- `clarification_answered`

Payloads should include enough metadata to distinguish recommended option answers from custom text.

### API Endpoints

Not applicable. This feature has no external HTTP API.

## Integration Points

No external integrations are required. The feature integrates only with existing in-process runtime contracts, app state, TUI rendering, Chat projection, and Session History.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/orchestrator/mod.rs` | Modified | Expands `OrchestratorDecision` and validation. Medium risk because every runtime parses this contract. | Add option model and validation tests. |
| `src/runtime/{codex,claude,cursor}.rs` | Modified | Prompt examples must include clarification options. Medium risk of schema drift. | Update embedded contracts and tests. |
| `src/runtime/fake.rs` | Modified | Fake clarification branch should return deterministic options. Low risk. | Add options for `needs clarification` scenario. |
| `src/app/mod.rs` | Modified | Adds public pending clarification view and dedicated answer path. Medium risk around resume behavior. | Mirror approval-style state exposure without approval semantics. |
| `src/tui/mod.rs` | Modified | Adds pending clarification composer mode and render/key handling. Medium risk around existing input cursor and dropdown behavior. | Reuse dropdown patterns while keeping custom text editable. |
| `src/app/chat/mod.rs` | Modified | Adds clarification item kind and waiting-user status. Low risk. | Extend enum slugs and status labels. |
| `src/app/chat/projection.rs` | Modified | Projects requested/answered events distinctly from approvals. Medium risk around lifecycle keys. | Add projection tests for pending and answered clarification. |
| `src/history/mod.rs` | Unchanged | Generic JSONL event store already supports new event payloads. Low risk. | No storage change required. |

## Testing Approach

### Unit Tests

- Orchestrator validation accepts 2-4 options and rejects missing, duplicate, or invalid recommended option ids.
- Runtime prompt contract tests verify Codex, Claude, and Cursor examples include clarification option fields.
- Fake runtime returns deterministic options for `needs clarification`.
- App tests verify `PendingClarificationView` is exposed, selected option answers resume the run, custom answers resume the run, and slash-prefixed custom answers still work.
- Chat projection tests verify clarification waits use `ChatItemKind::Clarification` and `WaitingForUser`, not approval.
- TUI tests verify rendering, Up/Down selection, Enter submission, custom text editing, and interrupt behavior while clarification is pending.

### Integration Tests

- Existing fake-runtime app flow should cover full pause, answer, resume, and complete behavior.
- Add one integration-style app test that reads Session History and asserts `clarification_requested` then `clarification_answered` payloads.

## Development Sequencing

### Build Order

1. **Shared clarification models** - no dependencies.
2. **Orchestrator schema and validation** - depends on step 1.
3. **Runtime prompt contract updates and fake runtime options** - depends on step 2.
4. **App pending clarification view and answer event** - depends on steps 1 and 2.
5. **History event payload updates** - depends on step 4.
6. **Chat kind/status and projection updates** - depends on step 5.
7. **TUI clarification composer mode** - depends on steps 4 and 6.
8. **Layered test completion and documentation notes** - depends on steps 2 through 7.

### Technical Dependencies

- No new external packages.
- No storage migration.
- No new runtime provider.
- No new API surface.

## Monitoring and Observability

- Record `clarification_requested` with `run_id`, `question_id`, option count, and recommended option id.
- Record `clarification_answered` with `run_id`, `question_id`, answer source, selected option id when present, and elapsed time if available.
- Track local metrics from Session History:
  - requested count;
  - answered count;
  - recommended versus custom answer ratio;
  - unresolved waiting runs;
  - answer latency.

## Technical Considerations

### Key Decisions

- **Decision:** Add structured option fields to `OrchestratorDecision`.  
  **Rationale:** The orchestrator owns the question and can provide meaningful answer options.  
  **Trade-off:** Runtime prompt contracts must be updated.

- **Decision:** Add `PendingClarificationView` and `AppEvent::ClarificationAnswered`.  
  **Rationale:** The TUI needs explicit state and the app needs structured answer metadata.  
  **Trade-off:** Adds app state surface area, but avoids inference from raw prompt submission.

- **Decision:** Add dedicated Chat semantics.  
  **Rationale:** The PRD requires clarification to be visually distinct from Action Approval.  
  **Trade-off:** Adds enum variants and projection work.

- **Decision:** Keep custom text visible below the option list.  
  **Rationale:** The user selected a low-friction custom answer path.  
  **Trade-off:** TUI layout must reserve space for both options and text.

### Known Risks

- **Runtime schema drift:** Multiple adapters embed contract examples. Mitigate with tests that assert the option fields are present.
- **TUI input conflicts:** Existing Up/Down keys move the input cursor or dropdown selection. Mitigate with a dedicated clarification mode that takes precedence when pending.
- **Approval/clarification confusion:** Current Chat uses approval-oriented status for waiting. Mitigate with dedicated Chat kind and status.
- **Poor option quality:** The app cannot fix bad recommendations. Mitigate by always exposing custom text and tracking answer source.

## Architecture Decision Records

- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — V1 upgrades existing orchestrator clarification instead of creating a broad agent question protocol.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — The PRD uses the focused clarification-select path and keeps any-agent questions out of V1.
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — The TechSpec uses structured orchestrator options, app-owned pending state, dedicated Chat semantics, inline custom text, and layered tests.
