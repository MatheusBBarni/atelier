---
status: completed
title: First-run nudge when the config-setup skill is absent
type: backend
complexity: low
dependencies:
  - task_01
---

# First-run nudge when the config-setup skill is absent

## Overview
Add an optional, read-only one-line tip at atelier startup that points users to install/invoke the config-setup skill when no `atelier-config-setup` skill is discovered. This is the discoverability backstop (PRD F7 / ADR-006) for the npm v12 install-script change — surfaced via first-run output rather than a lifecycle script — and must be unobtrusive and suppressible.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST show a single-line hint at startup ONLY when discovery finds no `atelier-config-setup` skill, pointing to the install/invoke path (`npx skills add MatheusBBarni/atelier atelier-config-setup` and/or `/skill:config-setup`).
- MUST be read-only — the nudge never writes to skill roots or config (consistent with F7's non-writing hint).
- MUST be suppressible and not spam: shown at most appropriately (e.g. gated by the existing banner/`hide_banner` posture or a show-once latch), and never when the skill is already discoverable.
- MUST derive "skill present?" from the real skills-discovery result, not a hardcoded path.
- SHOULD reuse the existing welcome/banner rendering path rather than introducing a parallel output channel.
</requirements>

## Subtasks
- [x] 7.1 Compute "is `atelier-config-setup` discoverable?" from the skills-discovery result at startup.
- [x] 7.2 Render a single-line nudge when absent, in the existing welcome/startup output path.
- [x] 7.3 Gate the nudge so it is suppressible and not shown when the skill is present.
- [x] 7.4 Add tests for present (no nudge) vs absent (nudge) and the suppression path.

## Implementation Note
Presence is derived from `ui_state.skill_suggestions` via a testable `config_setup_skill_present()`
helper (read-only, not a hardcoded path). The nudge is a one-line, theme-muted fact rendered in
`welcome::welcome_lines` only when the skill is absent, and suppressed under `hide_banner` (the
suppression lever). Tests: `config_setup_nudge_shows_only_when_skill_absent`,
`config_setup_nudge_suppressed_by_hide_banner` (welcome.rs), and
`config_setup_skill_present_reflects_discovery` (mod.rs).

## Follow-up (out of scope)
Pre-existing, environment-sensitive failure observed (NOT introduced by this task; reproduces with
this task's changes stashed): `tui::tests::reload_skills_command_refreshes_cache_and_clears_input`.
Root cause is `refresh_skill_suggestions` → `discover_skill_suggestions(roots).unwrap_or_default()`
returning empty when discovery over the developer's real HOME skill roots (`~/.agents/skills` /
`~/.claude/skills`) errors. Worth hardening separately (e.g. degrade per-root instead of dropping all
suggestions on any error), but it is unrelated to the config-setup skill.

## Implementation Details
Wire into the TUI startup/welcome path (`src/tui/welcome.rs` / `src/tui/mod.rs` around the banner + `hide_banner` handling, ~640/726). Query the skills-discovery module (`src/skills/mod.rs`) for the presence of `atelier-config-setup`. Keep it a small, additive, read-only line; respect the existing `[ui] hide_banner` posture for suppression. See TechSpec "System Architecture → First-run nudge" and ADR-006 (hint via README + first-run nudge).

### Relevant Files
- `src/tui/welcome.rs` — banner/welcome rendering (synthetic welcome chat item).
- `src/tui/mod.rs` — `hide_banner = config.ui.hide_banner` (~640), `ui_state` (~726) — where startup tips are gated.
- `src/skills/mod.rs` — discovery/suggestion API to query for `atelier-config-setup`.
- `src/config/mod.rs` — `UiConfig` (`hide_banner`, ~259) for the suppression posture.

### Dependent Files
- None — additive, read-only startup behavior.

### Related ADRs
- [ADR-006: Hint via README + first-run nudge (npm v12 install-script change)](../adrs/adr-006.md)
- [ADR-001: Non-writing discoverability backstop (F7)](../adrs/adr-001.md)

## Deliverables
- A suppressible, read-only one-line startup nudge shown only when `atelier-config-setup` is not discovered.
- Unit tests **(REQUIRED)** with ≥80% coverage of the nudge decision logic.

## Tests
- Unit tests:
  - [ ] Nudge text is produced when discovery reports `atelier-config-setup` absent.
  - [ ] No nudge is produced when the skill is discoverable.
  - [ ] The nudge is suppressed under the configured suppression posture (e.g. `hide_banner`/show-once).
- Integration tests:
  - [ ] Startup render in a temp root WITHOUT the skill contains the one-line hint; WITH the skill present it does not. (reuse TUI render-to-text patterns)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- The nudge appears only when the skill is missing, is one line, read-only, and suppressible.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
