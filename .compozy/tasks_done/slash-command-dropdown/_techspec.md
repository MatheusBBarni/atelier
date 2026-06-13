# TechSpec: Slash Command Dropdown

## Executive Summary

Implement the slash-command dropdown as a TUI-centered enhancement backed by a shared metadata-only command catalog. The catalog lives in `src/slash_commands.rs` and feeds dropdown rows, TUI help text, and app unknown-command guidance. Existing command execution stays unchanged: TUI-local commands remain in the TUI, app commands remain in `App::submit_prompt`, and `/agent:` plus `/skill:` remain prompt prefixes.

Primary trade-off: this avoids a risky command-dispatch refactor, but requires discipline to keep the metadata catalog aligned with command behavior.

## System Architecture

### Component Overview

- `src/slash_commands.rs`: shared fixed V1 command metadata.
- `src/tui/mod.rs`: command dropdown state, filtering, rendering, keyboard handling, insertion, and prefix handoff.
- `src/app/mod.rs`: unknown-command guidance generated from shared metadata; existing command handlers unchanged.
- Existing `/agent:` and `/skill:` dropdowns: keep precedence over the new command dropdown after those prefixes are active.

Data flow:

1. TUI reads command specs from `slash_commands`.
2. TUI filters specs when input starts with `/` at char position 0.
3. TUI renders suggestions or compact no-match state above the input.
4. `Tab`/`Enter` inserts selected `insert_text`; no app event is dispatched.
5. App uses the same specs to format unknown-command guidance.

## Implementation Design

### Core Interfaces

```rust
pub struct SlashCommandSpec {
    pub label: &'static str,
    pub insert_text: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
    pub kind: SlashCommandKind,
}

pub enum SlashCommandKind {
    TuiLocal,
    AppCommand,
    PromptPrefix,
}
```

```rust
struct CommandDropdown {
    query: String,
    suggestions: Vec<&'static SlashCommandSpec>,
    selected: Option<usize>,
    empty: bool,
}
```

### Data Models

- `SlashCommandSpec`: metadata only. No callbacks, dispatch functions, runtime state, or permissions.
- Fixed catalog entries: `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, `/reload:skills`.
- `TuiUiState` additions:
  - `command_selection_index: usize`
  - optional command dropdown dismissal state keyed by current query/input, used for Escape.
- `TuiCommand` addition:
  - `CommandDropdown(DropdownCommand)`
  - optional `DismissCommandDropdown`

No persistent storage, database schema, network API, or external integration is required.

### API Endpoints

Not applicable. This is a local TUI/app behavior change.

## Integration Points

- `src/tui/mod.rs`
  - Extend dropdown precedence: help modal, agent dropdown, skill dropdown, command dropdown, normal input.
  - Render command dropdown using the same visual pattern as agent/skill dropdowns.
  - Add `Tab` as an accept key for command dropdown only.
  - Trap `Enter` when the command dropdown is in the no-match state.
- `src/app/mod.rs`
  - Replace hardcoded available-command text in `reject_unknown_slash_command` with catalog-derived guidance.
  - Keep existing `/goal`, `/config`, `/subtask`, `/agent:`, `/skill:` handling unchanged.
- `README.md`
  - Documentation may remain manual, but command list should be reviewed after implementation.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
| --- | --- | --- | --- |
| `src/slash_commands.rs` | new | Shared metadata catalog; low risk if metadata-only | Add fixed V1 specs and formatting helpers |
| `src/tui/mod.rs` | modified | New dropdown state and key routing; medium risk around input behavior | Add command dropdown detection, rendering, acceptance, no-match handling |
| `src/app/mod.rs` | modified | Unknown-command guidance changes; low risk | Consume catalog for available-command text |
| TUI tests | modified/new | Existing agent/skill dropdown behavior must remain stable | Add focused command dropdown tests |
| App tests | modified/new | Unknown-command guidance must include fixed V1 commands | Add catalog/guidance assertions |

## Testing Approach

### Unit Tests

- `slash_commands` catalog contains exactly the fixed V1 command set.
- Unknown-command guidance includes all fixed V1 commands, including `/reload:skills`.
- `/` renders command dropdown with all fixed commands.
- `/g` filters to `/goal` and `/goal clear`.
- Up/Down cycles selection.
- `Tab` and `Enter` accept selected suggestions without dispatching an app event.
- Accepting `/agent:` or `/skill:` immediately hands off to existing specialized dropdown behavior.
- No command dropdown appears during pending approval or `WaitingForUser`.
- Unmatched slash input renders "No commands found".
- `Enter` is trapped while no-match state is visible.

### Integration Tests

No broad runtime integration tests are required for V1. Existing app slash-command tests should continue to cover `/goal`, `/config`, `/subtask`, named prompt prefixes, and clarification answers that start with `/`.

## Development Sequencing

### Build Order

1. Add `src/slash_commands.rs` and export it from the crate root - no dependencies.
2. Update `src/app/mod.rs` unknown-command guidance to use the catalog - depends on step 1.
3. Update TUI help command rows to use catalog metadata - depends on step 1.
4. Add command dropdown state, query detection, filtering, and empty-state model in `src/tui/mod.rs` - depends on step 1.
5. Add command dropdown rendering using the existing agent/skill dropdown visual approach - depends on step 4.
6. Add key routing for Up/Down, `Tab`, `Enter`, Escape, and no-match trapping - depends on steps 4 and 5.
7. Add prefix handoff for `/agent:` and `/skill:` accepted from the command dropdown - depends on step 6.
8. Add focused unit tests for catalog, app guidance, TUI rendering, filtering, acceptance, no-match, gating, and handoff - depends on steps 1 through 7.

### Technical Dependencies

- Existing Ratatui/Crossterm TUI stack.
- Existing char-index helpers for cursor-safe text replacement.
- Existing app run-state and pending-approval state.

## Monitoring and Observability

No new runtime telemetry is required for V1. Success can be verified through focused tests and existing error/diagnostic behavior. If product metrics are added later, track unknown slash-command submissions and command dropdown acceptances as separate events.

## Technical Considerations

### Key Decisions

- Decision: shared metadata-only catalog in `src/slash_commands.rs`.
  - Rationale: aligns visible command metadata without changing execution boundaries.
  - Trade-off: metadata can still drift from behavior unless tests enforce catalog usage.
  - Alternatives rejected: full dispatch registry, TUI-local metadata, app-scoped metadata.

- Decision: command dropdown activates only at input start.
  - Rationale: reduces accidental activation on paths, URLs, and inline slash text.
  - Trade-off: no mid-prompt command discovery in V1.
  - Alternatives rejected: after-whitespace activation, anywhere-after-slash activation.

- Decision: no-match `Enter` is trapped.
  - Rationale: prevents the invalid submission the feature is meant to reduce.
  - Trade-off: stricter than normal text submission.
  - Alternatives rejected: raw submission on no match, clearing unmatched text.

### Known Risks

- Existing app command order allows recognized commands before clarification handling. The TUI gating prevents dropdown interference, but direct submission behavior remains unchanged.
- Escape dismissal needs lightweight state; without it, the dropdown may immediately reappear.
- `/goal` has both a no-argument command and text argument usage. Accepting `/goal` should insert command text and dismiss dropdown so subsequent goal text is normal input.
- Command row descriptions may overflow narrow terminals. Reuse existing truncation patterns from skill dropdown rows.

## Architecture Decision Records

- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) - Proceed with a state-aware discovery dropdown backed by narrow shared command metadata.
- [ADR-002: Choose Error-Reduction Product Approach](adrs/adr-002.md) - Optimize V1 for fewer unknown-command submissions.
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) - Add `src/slash_commands.rs` as a shared metadata catalog while preserving execution ownership.
- [ADR-004: Scope Slash Dropdown Activation And Keyboard Semantics](adrs/adr-004.md) - Activate only at input start, preserve prefix handoff, and trap no-match `Enter`.
