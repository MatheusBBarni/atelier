---
status: pending
title: "Lean Markdown transcript serializer"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 03: Lean Markdown transcript serializer

## Overview
Add `src/app/chat/markdown.rs` with `render_session_markdown(preview, scan)`, a pure function that turns a `SessionPreview` into the lean, summary-first Markdown artifact: a TL;DR and redaction summary up top, verbose spans collapsed in HTML `<details>`, and flagged spans surfaced with explicit labels. This is the consumer-facing shape of the transcript.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `pub fn render_session_markdown(preview: &SessionPreview, scan: &[SecretFinding]) -> String` in a new `src/app/chat/markdown.rs`, declared `pub mod markdown;` in `src/app/chat/mod.rs`.
- MUST lead with a TL;DR (files changed, commands, run outcome) and a redaction summary (counts by `SecretCategory`), per the TechSpec "Data Models" artifact structure.
- MUST wrap verbose bodies (file reads, command output) in `<details><summary>…</summary>` and keep flagged spans **expanded**, labelled `⚠ **redacted**: <category>` using the `<redacted>` placeholder convention.
- MUST render an unknown `ChatItemKind` via a generic title/body fallback — never drop an item.
- MUST NOT mutate kept diff/command/reasoning text except replacing flagged spans / credential-file bodies with labels (detect-don't-mutate).
- MUST scrub `ChatDetailRef::Inline.content` through the scan before emitting it.
</requirements>

## Subtasks
- [ ] 3.1 Create the module and declare `pub mod markdown`.
- [ ] 3.2 Render the header: TL;DR + redaction summary.
- [ ] 3.3 Render per-item sections keyed on `ChatItemKind`, collapsing verbose kinds in `<details>`.
- [ ] 3.4 Apply flagged-span labelling from `scan` findings; keep flagged spans visible.
- [ ] 3.5 Add the generic fallback for unknown kinds and the best-effort footer notice.

## Implementation Details
Create `src/app/chat/markdown.rs` as a sibling of `diff_preview.rs`/`command_summary.rs`. It consumes the already-sanitized `SessionPreview.items` (from `build_session_preview`) and the `SecretFinding`s from task_02; it does NOT call the diff/command preview builders directly (their output is already folded into `ChatItemView.body`/`details`). See TechSpec "Implementation Design" for the artifact structure.

### Relevant Files
- `src/app/chat/mod.rs` — `ChatItemView` and enums (`ChatItemKind`, `ChatLineStyle`, `ChatDetailRef`, `ChatSeverity`), `SessionPreview`, `sanitize_transcript_text`.
- `src/app/chat/diff_preview.rs`, `src/app/chat/command_summary.rs` — sibling pure-render helpers (pattern reference).
- `src/runtime/status.rs` — `SecretFinding`/`SecretCategory` (task_02).

### Dependent Files
- `src/export.rs` (task_04) — calls `render_session_markdown`.
- `src/app/chat/mod.rs` — add `pub mod markdown;`.

### Related ADRs
- [ADR-002: Lean review artifact](../adrs/adr-002.md) — summary-first, `<details>` collapse, flagged spans surfaced.
- [ADR-003: Component architecture](../adrs/adr-003.md) — serializer placement in `app::chat`.

## Deliverables
- `src/app/chat/markdown.rs` with `render_session_markdown` + module wiring.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration coverage of the rendered file is provided in task_06 **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A preview with one prompt + one edit renders a TL;DR line and an `## Edit` section.
  - [ ] A long file-read body is wrapped in `<details><summary>`.
  - [ ] A finding overlapping an item body renders `⚠ **redacted**: provider_token` with the `<redacted>` label, and the secret bytes are NOT emitted verbatim.
  - [ ] The redaction summary shows correct counts by category (e.g. 1 provider_token, 3 high_entropy).
  - [ ] An item with an unknown/forced kind still renders a titled section (no drop).
  - [ ] Output contains the best-effort "rotate credentials" footer notice.
- Integration tests:
  - [ ] (covered in task_06) the exported file contains the prompt and a `<details>` block and excludes seeded secrets.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Output is summary-first with `<details>` collapse; flagged spans labelled and never emitted verbatim
- Unknown item kinds preserved via fallback
