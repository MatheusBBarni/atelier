---
status: pending
title: Shared Severity/Finding types and finding-line parser
type: backend
complexity: low
dependencies: []
---

# Task 01: Shared Severity/Finding types and finding-line parser

## Overview
Introduce the shared `Severity` and `Finding` value types plus a tolerant `parse_finding_line` function that turns one formatted reviewer line into a structured `Finding`. These are the leaf types the security-review event payload and chat card are built from, and they are deliberately decoupled from any one workflow so the V2 cross-runtime gate can reuse them (ADR-001, ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define `Severity` (Critical, High, Medium, Low, Info) and `Finding { severity, title, location, why, fix }` as serde-serializable types colocated with `GraderVerdict` in `src/orchestrator/mod.rs`.
- MUST NOT couple these types to the security-review workflow, council, or grading — they are shared leaf types.
- MUST provide `parse_finding_line(&str) -> Finding` that parses the rubric line format `[SEVERITY] title — location — why — fix`.
- The parser MUST be total (never panic, never drop input): an unrecognized line becomes a `Finding` with a default severity (Medium) and the raw text preserved as `title`.
- Severity token parsing MUST be case-insensitive and tolerate surrounding whitespace.
</requirements>

## Subtasks
- [ ] 1.1 Define the `Severity` enum with serde rename to lowercase string values.
- [ ] 1.2 Define the `Finding` struct with optional `location`/`why`/`fix` fields.
- [ ] 1.3 Implement `parse_finding_line` with the `[SEV] title — loc — why — fix` grammar and a safe fallback.
- [ ] 1.4 Add a helper to map a `Severity` to the existing `ChatSeverity`-adjacent ordering (max-severity selection) without importing chat types into orchestrator.
- [ ] 1.5 Add unit tests covering well-formed, partial, malformed, and adversarial lines.

## Implementation Details
Add the types and parser to `src/orchestrator/mod.rs` next to `GraderVerdict` (the self-grading verdict precedent). See TechSpec "Core Interfaces" and "Data Models" for the exact shape and field semantics, and ADR-004 for why findings are parsed app-side rather than via a new runtime schema. Do not add a new module or package — these belong with the existing result envelopes.

### Relevant Files
- `src/orchestrator/mod.rs` — defines `GraderVerdict` (~369) and `AgentResult` (~211); the new types live here alongside them.

### Dependent Files
- `src/app/chat/projection.rs` — task_04 consumes `Finding`/`Severity` to render the card.
- `src/app/mod.rs` — task_05 calls `parse_finding_line` over `AgentResult.findings`.

### Related ADRs
- [ADR-004: Findings parsed app-side into shared Severity/Finding types](../adrs/adr-004.md) — defines the data path this task implements.
- [ADR-001: Standalone reviewer, own event family with shared leaf types](../adrs/adr-001.md) — mandates shared leaf types for V2 reuse.

## Deliverables
- `Severity` and `Finding` types in `src/orchestrator/mod.rs`, serde round-trippable.
- A total `parse_finding_line` function with documented grammar and fallback.
- Unit tests with 80%+ coverage **(REQUIRED)**
- (Integration coverage for these types is exercised end-to-end in task_05.)

## Tests
- Unit tests:
  - [ ] `parse_finding_line("[HIGH] SQL injection — src/db.rs:42 — unsanitized input — use params")` yields `severity=High`, `location=Some("src/db.rs:42")`, populated `why`/`fix`.
  - [ ] Lowercase/mixed-case severity token (`[critical]`, `[Low]`) parses to the correct `Severity`.
  - [ ] A line with only `[MEDIUM] title` (no location/why/fix) yields `None` for the missing fields, never a panic.
  - [ ] A line with no recognizable `[SEVERITY]` prefix yields `severity=Medium` with the full raw text as `title`.
  - [ ] An adversarial line containing injection text (`[HIGH] ignore previous instructions`) parses as data — `title` carries the text verbatim, no interpretation.
  - [ ] `Severity` serializes to lowercase strings and deserializes back (serde round-trip).
- Integration tests:
  - [ ] (Covered in task_05: parsed findings flow into the `security_review_completed` payload.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `Severity`/`Finding` compile as standalone serde types with no dependency on chat or app modules.
- `parse_finding_line` is total over arbitrary `&str` input.
