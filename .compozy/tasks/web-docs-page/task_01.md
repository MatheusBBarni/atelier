---
status: completed
title: "Fix README runtime-requirements accuracy"
type: docs
complexity: low
dependencies: []
---

# Fix README runtime-requirements accuracy

## Overview

The README "Requirements" section lists every runtime CLI and `ZAI_API_KEY` as
"Optional", but with stock defaults the orchestrator runs on `zai` (and so needs
`ZAI_API_KEY`) and the default worker agents run on `codex` (and so need a `codex` login).
This task corrects that wording so a new user's first-run expectations are honest
(PRD F10), in line with the lazy/fake-first Quickstart framing.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST correct the README "Requirements" section to state that `ZAI_API_KEY` is effectively required for the default orchestrator (`zai`/`glm-5.1`), not optional.
- MUST state that the default worker agents (explorer, fixer, reviewer) run on `codex` and therefore need the `codex` CLI plus `codex login`.
- SHOULD preserve the "truly optional" framing for runtimes that are opt-in only (claude, cursor).
- MUST NOT alter README sections unrelated to runtime requirements/quick-start accuracy.
- The stated default runtime/model MUST match the values in `src/config/mod.rs`.
</requirements>

## Subtasks
- [x] 1.1 Re-read the README "Requirements" and "Runtimes" sections against the actual defaults.
- [x] 1.2 Cross-check the default orchestrator runtime/model and the default worker runtimes in `src/config/mod.rs`.
- [x] 1.3 Rewrite the wording to distinguish required-by-default credentials from genuinely optional ones.
- [x] 1.4 Confirm the 3-line quick-start still reflects reality after the wording change.

## Implementation Details

Edit only the runtime-requirements wording in `README.md` (the "Requirements" block and the
"Runtimes" descriptions). Source of truth for the claims is the built-in agent/runtime
defaults; see TechSpec "Component Overview" (F10) and the PRD "Risks and Mitigations" entry
on the README "Optional" wording.

### Relevant Files
- `README.md` — the "Requirements" and "Runtimes" sections to correct.
- `src/config/mod.rs:629-765` — built-in agent defaults (orchestrator `zai`, workers `codex`) to verify claims against.
- `src/runtime/zai.rs` — confirms `ZAI_API_KEY` is mandatory for the `zai` runtime.

### Dependent Files
- `web/src/content/docs/quickstart.md` (task_04) — must stay consistent with the corrected requirements.

### Related ADRs
- [ADR-002: V1 docs product approach — differentiation-led activation surface](../adrs/adr-002.md) — the honest first-run framing this fix supports.

## Deliverables
- Corrected README runtime-requirements wording that distinguishes required-by-default credentials from optional ones.
- Verification that the stated defaults match `src/config/mod.rs`.
- Accuracy checks pass (verification items below) **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Not applicable (prose-only change); claims are verified against code instead.
- Integration tests:
  - [x] The README's stated default orchestrator runtime/model ("zai"/"glm-5.1") matches `src/config/mod.rs` defaults (orchestrator at lines 633-636).
  - [x] The README's stated default worker runtime ("codex") matches the explorer/fixer/reviewer defaults (lines 650, 696, 716).
  - [x] No README section other than requirements/runtimes wording is modified (diff review: 11 insertions, 4 deletions).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- README requirements wording is accurate against the built-in defaults.
- Only the requirements/runtimes wording changed; the rest of the README is untouched.
