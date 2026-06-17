---
status: completed
title: Docs & recipes
type: docs
complexity: low
dependencies:
  - task_02
  - task_03
  - task_07
  - task_08
---

# Task 9: Docs & recipes

## Overview
Document the hooks feature for users: a README features line, a `[hooks]` configuration section, three copy-paste recipes (notify, append-to-audit-file, webhook), the `--events follow` and `--doctor` surfaces, and the tmux passthrough note. Bundled recipes and discoverability are the adoption levers the research identified.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `[hooks]` subsection to the README "Configuration" area documenting the `[[hooks.handler]]` schema (`on`, `notify`, `command`, `payload`, `notify_fallback_command`).
- MUST include three copy-paste recipes: desktop notify, append-to-audit-file, and webhook POST.
- MUST document `atelier --events follow` (dry-run/test harness) and the `--doctor` hooks signal.
- MUST document the SSH/tmux behavior: OSC-native default and the `notify_fallback_command` + tmux passthrough note.
- MUST add a one-line hooks entry to the README "Features" list.
- SHOULD confirm `/config` surfaces active hooks (note the path; no slash-command catalog change is required since hooks are passive).
</requirements>

## Subtasks
- [x] 9.1 Add the README "Features" one-liner and the `[hooks]` configuration subsection.
- [x] 9.2 Write the three recipes (notify, audit-file, webhook).
- [x] 9.3 Document `--events follow` and the `--doctor` hooks check.
- [x] 9.4 Document the SSH/tmux notifier behavior and fallback.
- [x] 9.5 Add a test asserting the documented config examples parse.

## Implementation Details
Edit `README.md`: the "Features" list (`:14-30`) and the "Configuration" section (`:160-189`); optionally the "CLI" list (`:86-103`) for `--events follow`. Keep examples consistent with the task_02 schema and the task_03/task_05 notifier behavior. To satisfy the test requirement for a docs task, add a unit test (in the config module or a docs test) that parses each documented `[[hooks.handler]]` example through the real config parser, guaranteeing the docs never drift from the schema. See PRD "User Experience" and ADR-004/ADR-005.

### Relevant Files
- `README.md:14` — "Features" list; add the hooks one-liner.
- `README.md:160` — "Configuration" section; add the `[hooks]` subsection + recipes.
- `README.md:86` — "CLI" list; add `--events follow`.

### Dependent Files
- `src/config/mod.rs` — the documented examples must parse against this schema (test).
- `src/slash_commands.rs` — no change (hooks are passive); `/config` already renders loaded config.

### Related ADRs
- [ADR-004: Handler-array config schema](../adrs/adr-004.md) — the documented schema.
- [ADR-005: Built-in notifier](../adrs/adr-005.md) — the SSH/tmux/fallback documentation.

## Deliverables
- README features line + `[hooks]` configuration section + three recipes.
- `--events follow` and `--doctor` documentation + SSH/tmux note.
- A test that the documented config examples parse **(REQUIRED)**
- Integration check: the documented starter config round-trips through the config loader **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Each documented `[[hooks.handler]]` recipe (notify, audit-file, webhook) parses successfully through the config loader. — `documented_hooks_examples_parse_through_the_loader` (extracts every README ```toml hooks block and loads it — anti-drift)
  - [x] The documented `notify_fallback_command` example parses into `HooksConfig`. — covered by the same extractor test (the fallback block is one of the loaded blocks) + `documented_hooks_block_round_trips_through_home_config`
- Integration tests:
  - [x] The full documented `[hooks]` block round-trips through home-config loading without error. — `documented_hooks_block_round_trips_through_home_config`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- The README documents the schema, three recipes, `--events follow`, `--doctor`, and SSH/tmux behavior
- Documented examples are verified to parse against the real config schema
- The "Features" list advertises lifecycle hooks

## As-built notes
- README gains a Features one-liner, a `--events follow` CLI entry + note, a `[hooks]` bullet in the config list, and a full `## Lifecycle hooks` section: security posture (home-only, stdin-only, redacted), schema, the 10 public event names, three recipes (notify / audit-to-file / webhook), the `--events follow` and `--doctor` surfaces, the `/config` note, and the OSC-default + tmux `allow-passthrough` + `notify_fallback_command` guidance.
- **Anti-drift test:** `documented_hooks_examples_parse_through_the_loader` `include_str!`s the README, extracts every ```toml fenced block containing `hooks`, and loads each through `load_effective_config` — so an invalid documented example fails the build. `documented_hooks_block_round_trips_through_home_config` asserts the combined recipes produce 3 handlers + the fallback. Both live in `config::tests` (the schema's home).
- Each documented block is independently loadable (the schema example shows only `[[hooks.handler]]`; the fallback `[hooks]` block is shown separately in the SSH section), avoiding array-then-table ordering ambiguity within a single snippet.
- No `src/slash_commands.rs` change: hooks are passive, and `/config` already renders the loaded config.
