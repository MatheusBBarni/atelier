---
status: completed
title: README install + usage documentation
type: docs
complexity: low
dependencies:
  - task_01
---

# README install + usage documentation

## Overview
Document the config-setup skill in `README.md`: how to install it into any agent's roots via `npx skills add`, the manual-copy fallback (skills.sh independence), and how to invoke it — plus a short greenfield-vs-import note so users don't confuse it with the future config-importer. This is the human-facing discovery surface that backstops adoption (PRD risks: "opt-in install nobody runs").

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a README section documenting installation via `npx skills add MatheusBBarni/atelier atelier-config-setup` (name-targeted to avoid mirror duplication), and that atelier's own runtime already bundles it.
- MUST document a manual-copy fallback (copy `skills/atelier-config-setup/` into a discovery root) so the skill works without skills.sh availability.
- MUST document invocation: `/skill:config-setup` (atelier/host agent) or "set up my atelier config", and that it self-validates with `atelier --print-config` / `--doctor` when available.
- MUST include a one-line greenfield-vs-import cross-reference (this skill builds a fresh config; importing existing tool conventions is the separate config-importer roadmap item — ADR-003).
- SHOULD state the secret posture (the skill sets `api_key_env` names; the user exports the key).
- MUST NOT introduce a version-number mismatch — README is prose; do not edit version strings (the `check:versions` gate, per the repo CLAUDE.md versioning rule, must stay green).
</requirements>

## Subtasks
- [x] 8.1 Add an "atelier-config-setup skill" subsection under the README install/usage area.
- [x] 8.2 Document `npx skills add` (name-targeted) + the repo-bundled-for-atelier note.
- [x] 8.3 Document the manual-copy fallback and invocation/self-validate flow.
- [x] 8.4 Add the greenfield-vs-import cross-reference and the secret posture line.

## Implementation Note
Added a `## Configure with the atelier-config-setup skill` section to `README.md` after Install.
Invocation documented as `/skill:atelier-config-setup` (the canonical skill name; the PRD's
`/skill:config-setup` shorthand predates the final name). Docs-presence guard added to the task_06
module: `readme_documents_install_and_invocation` asserts the install command, skill name, and an
invocation form are present. `check:versions` stays green (no version strings touched).

## Implementation Details
Edit `README.md` near the existing `## Install` section (~line 47). Keep it concise and accurate to the install command resolution verified during build (the `npx skills` recursive-scan caveat → name-targeted install, ADR-004 risk note). See TechSpec "Impact Analysis (README.md)" and PRD "User Experience" (discover → install → invoke). No code; documentation only.

### Relevant Files
- `README.md` — `## Install` section (~47) and usage area.
- `skills/atelier-config-setup/SKILL.md` (task_01) — the skill being documented (invocation/behavior must match).
- `src/cli.rs` — `--init-config` / `--print-config` referenced in the self-validate note.

### Dependent Files
- None — documentation only.

### Related ADRs
- [ADR-006: Distribute via `npx skills add` (git); hint via README + first-run nudge](../adrs/adr-006.md)
- [ADR-002: skills.sh convention + repo bundle (manual-copy fallback)](../adrs/adr-002.md)
- [ADR-003: Greenfield vs the separate config-importer](../adrs/adr-003.md)

## Deliverables
- A README section: install (`npx skills add` + manual fallback), invoke, self-validate, secret posture, and greenfield-vs-import note.
- Tests **(REQUIRED)**: a docs-presence assertion (in task_06 or a small README check) that the install command + skill name appear, so the doc can't silently drop them.

## Tests
- Unit tests:
  - [ ] (Docs check) `README.md` contains the `npx skills add` command and the `atelier-config-setup` skill name. (assert in the task_06 module or a small dedicated test)
- Integration tests:
  - [ ] (Manual) Follow the README to install via `npx skills add` and confirm the skill is discoverable — recorded as a manual verification step, not CI (skills.sh is external).
- Test coverage target: >=80% (of the asserted doc invariants)
- All tests must pass

## Success Criteria
- All tests passing
- A new user can discover, install (or use the bundled copy / manual fallback), and invoke the skill from the README alone.
- `cargo fmt --check`, `cargo clippy --all-targets`, and `npm --prefix npm run check:versions` stay green.
