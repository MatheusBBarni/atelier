# PRD: Slash Command Dropdown

## Overview

Add a slash-command dropdown to Atelier's TUI composer so new and occasional users can discover valid commands before submitting invalid input. When the user types `/`, V1 shows the fixed command list, filters as the user types, and lets the user insert a selected command with `Tab` or `Enter`.

The feature prioritizes reducing unknown-command errors. It does not execute commands from the dropdown, broaden command scope, or replace existing `/agent:` and `/skill:` dropdown behavior.

## Goals

- Reduce unknown slash-command submissions by 60% in sessions where users type `/`.
- Make 100% of the fixed V1 command set visible from `/`.
- Preserve user control: accepted suggestions insert text only and never auto-submit.
- Match the existing skill and agent dropdown interaction model.
- Keep command names and descriptions aligned across dropdown, help, and unknown-command guidance.

## User Stories

- As a new Atelier user, I want typing `/` to show valid commands so I do not need to memorize them.
- As an occasional user, I want short command descriptions so I can choose the right command before submitting.
- As a user typing an invalid command, I want a compact no-match state so I know the input is not recognized.
- As a keyboard user, I want Up/Down plus `Tab` or `Enter` selection so I can stay in flow.
- As a user answering a clarification, I want slash-prefixed text to remain normal input so paths like `/tmp/project` still work.

## Core Features

- **Fixed V1 command suggestions:** Show only `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/workflow`, `/queue`, `/agent:`, `/skill:`, and `/reload:skills`. (`/workflow` and `/queue` were added by an approved 2026-06-12 scope amendment — see ADR-001 — because they shipped as real app commands after the original freeze; the set remains fixed and closed.)
- **Filter as typed:** Narrow visible suggestions as users type after `/`.
- **Compact empty state:** Show "No commands found" when no command matches.
- **Keyboard navigation:** Use Up/Down for selection and `Tab` or `Enter` to accept.
- **Text-only acceptance:** Insert or complete the selected command text without submitting it.
- **Safe activation:** Do not interfere with clarification or approval input.
- **Prefix continuity:** Preserve existing `/agent:` and `/skill:` dropdown behavior once those prefixes are active.
- **Metadata alignment:** Keep dropdown descriptions, help text, and unknown-command guidance consistent.

## User Experience

1. User opens the TUI composer and types `/`.
2. A dropdown appears above the input using the same visual approach as the current agent and skill dropdowns.
3. The list shows command names plus short descriptions.
4. Typing filters the list.
5. Up/Down changes the selected row.
6. `Tab` or `Enter` inserts the selected command text and keeps the prompt editable.
7. Escape or continued non-command input dismisses the dropdown without changing raw input.
8. If no command matches, the dropdown shows a compact empty state.

## High-Level Technical Constraints

- The dropdown must use the same user-facing interaction approach as the existing skill and agent dropdowns.
- The fixed V1 command set must stay limited to the approved commands.
- Slash-prefixed clarification answers must remain valid user input.
- Command metadata visible to users must stay aligned across the main command surfaces.

## Non-Goals

- User-defined or project-defined commands.
- Full command palette behavior.
- Fuzzy ranking, aliases, categories, or recent-command ranking.
- Disabled reasons, permission badges, or command previews.
- Direct execution from the dropdown.
- Changes to existing command semantics.

## Phased Rollout Plan

### MVP (Phase 1)

Ship the fixed V1 dropdown, filtering, compact empty state, Up/Down navigation, `Tab`/`Enter` insertion, safe activation, and prefix continuity.

Success criteria: the fixed command set is discoverable from `/`, selection inserts text only, and slash-command errors decrease.

### Phase 2

Add richer user-facing command guidance if MVP usage shows users still choose the wrong command.

Success criteria: users can identify the right command without leaving the composer.

### Phase 3

Consider broader command palette capabilities such as categories, aliases, recent ranking, disabled reasons, and custom commands.

Success criteria: expanded behavior improves command use without increasing confusion or accidental execution.

## Success Metrics

- Unknown slash-command submissions decrease by 60%.
- 100% of fixed V1 commands appear in the dropdown.
- 100% of accepted suggestions insert text without auto-submit.
- Keyboard flow supports show, filter, Up/Down, `Tab`, `Enter`, Escape, and no-match behavior.
- Help text, unknown-command guidance, and dropdown descriptions stay consistent for the fixed command set.

## Risks and Mitigations

- **Input theft:** Slash is also used in paths and literal text. Mitigation: preserve raw input and avoid activation during waiting states.
- **No-match noise:** Empty states can distract. Mitigation: keep the message compact.
- **Wrong-command selection:** Short descriptions may be too terse. Mitigation: monitor confusion and defer richer guidance to Phase 2.
- **Metadata drift:** Different surfaces can disagree. Mitigation: treat visible command metadata as one product surface.
- **Scope creep:** Command palettes can grow quickly. Mitigation: keep V1 fixed and explicitly defer richer behavior.

## Architecture Decision Records

- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) - Proceed with a state-aware discovery dropdown backed by narrow shared command metadata.
- [ADR-002: Choose Error-Reduction Product Approach](adrs/adr-002.md) - Optimize V1 for fewer unknown-command submissions.

## Open Questions

None for V1.

## References

- Claude Code interactive mode: <https://code.claude.com/docs/en/interactive-mode>
- Cursor slash commands changelog: <https://cursor.com/changelog/1-6>
- Slack shortcuts: <https://slack.com/help/articles/360057554553-Use-shortcuts-to-take-actions-in-Slack>
- W3C combobox pattern: <https://www.w3.org/WAI/ARIA/apg/patterns/combobox/>
