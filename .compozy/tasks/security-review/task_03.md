---
status: pending
title: Built-in security-reviewer agent and rubric
type: backend
complexity: low
dependencies: []
---

# Task 03: Built-in security-reviewer agent and rubric

## Overview
Register a built-in `security-reviewer` agent profile that is read-only by construction (no `Command`, no `Edit`) and carries the security-review methodology as inline instructions: the vulnerability-class checklist, severity scale, confidence gate, hard-exclusion list, untrusted-content framing, and the `[SEVERITY] title — location — why — fix` finding-line format. Shipping the rubric as built-in instructions guarantees it is always present regardless of workspace skill files (ADR-001, ADR-002, ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a built-in agent profile `security-reviewer` constructed in `src/config/mod.rs` via the existing built-in-agent insertion path.
- The profile MUST declare `capabilities = [Read]` only — explicitly NO `Command`, `Edit`, `WriteFile`, or `ApplyPatch`.
- MUST carry a capable default `model` plus a `model_fallbacks` chain so it is usable runtime-agnostically; emit/support a weak-model warning signal (consumed downstream by the card, F6).
- MUST define the rubric as an inline instruction constant covering: vulnerability classes, severity scale, confidence gate (drop low-confidence), hard-exclusion list (DoS/rate-limiting/open-redirect/already-secured-secrets/generic-input-validation), untrusted-content framing, and the exact finding-line output format.
- The rubric MUST instruct the model to emit each finding as one `findings` line in the `[SEVERITY] title — location — why — fix` format that task_01's parser expects.

## Subtasks
- [ ] 3.1 Author the `SECURITY_REVIEWER_RUBRIC` instruction constant (methodology + output-line contract + untrusted-content framing).
- [ ] 3.2 Register the built-in `security-reviewer` profile with read-only capabilities and a default model + fallbacks.
- [ ] 3.3 Ensure the profile is discoverable by id from the merged config like other built-in agents.
- [ ] 3.4 Add tests asserting the profile's capabilities and registration.

## Implementation Details
Add the profile alongside other built-in agents in `src/config/mod.rs` (the `insert_builtin_agent` path and the built-in agent constructors), mirroring the existing read-only "reviewer" template. The rubric content operationalizes ADR-003's curated report and ADR-002's untrusted-content framing, and the output-line format is the contract task_01 parses. See TechSpec "System Architecture → Reviewer agent" and "Technical Considerations → Key Decisions". Do not wire dispatch here — task_05 consumes this profile.

### Relevant Files
- `src/config/mod.rs` — `AgentProfile` (~480), `Capability` enum, `insert_builtin_agent` (~2248), built-in agent constructors (~1211).

### Dependent Files
- `src/app/mod.rs` — task_05 looks up the `security-reviewer` profile to build the runtime request.

### Related ADRs
- [ADR-004: Rubric as built-in instructions](../adrs/adr-004.md) — why inline, not a discovered skill.
- [ADR-002: Hostile-input boundary](../adrs/adr-002.md) — read-only capabilities + untrusted-content framing.
- [ADR-001: Standalone read-only reviewer](../adrs/adr-001.md) — capability restriction as a structural guarantee.

## Deliverables
- Built-in `security-reviewer` profile with `capabilities = [Read]` and a default model + fallbacks.
- `SECURITY_REVIEWER_RUBRIC` instruction constant.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration coverage of dispatch is exercised in task_05 **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The merged config exposes an agent with id `security-reviewer`.
  - [ ] Its capabilities contain `Read` and contain neither `Command` nor `Edit`.
  - [ ] It has a non-empty `model` and a non-empty `model_fallbacks` chain.
  - [ ] The rubric constant contains the finding-line format token and the hard-exclusion list keywords.
- Integration tests:
  - [ ] (Covered in task_05: the profile drives a runtime step that returns findings.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The `security-reviewer` profile is read-only by construction (capability test proves no Command/Edit).
- The rubric is present in the binary independent of any workspace skill file.
