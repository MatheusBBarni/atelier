# Slash Command Dropdown

## Overview

Add a state-aware slash-command dropdown to Atelier's TUI composer when a user starts typing `/`. V1 optimizes for new and occasional users by making available commands discoverable at the point of input, without turning the composer into a full command palette.

The MVP should show matching command names and short descriptions, support Up/Down navigation, support Tab and Enter acceptance, and preserve the existing `/agent:` and `/skill:` dropdown behavior. It should use narrow shared command metadata so help text, unknown-command guidance, and dropdown rows do not drift.

## Problem

Atelier has useful slash commands, but command discovery is split across README documentation, help text, and unknown-command errors. New or occasional users must already know that commands exist or make a mistake before seeing guidance.

The current TUI already proves that dropdown completion works for `/agent:` and `/skill:`. The missing piece is first-level command discovery from `/`, where users naturally expect the product to reveal available actions.

### Market Data

Modern agent CLIs now treat slash-command discovery as standard. Claude Code documents that typing `/` shows available commands and typing letters filters them. Cursor added custom slash commands in September 2025, invoked by typing `/` in Agent input and selecting from a dropdown. Slack's mainstream composer uses `/` to open a searchable shortcuts menu. W3C combobox guidance supports the same interaction model: Up/Down navigation, Enter acceptance, and Escape dismissal.

## Summary / Differentiator

The differentiator is not basic autocomplete. Atelier can make slash discovery safer and more trustworthy by keeping it state-aware, preserving raw input, and grounding visible command rows in canonical metadata.

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| TUI composer | Show a dropdown above the input when `/` starts a safe top-level command token. |
| `/agent:` and `/skill:` dropdowns | Preserve specialized dropdown precedence once those prefixes are active. |
| App slash commands | Keep existing execution ownership for `/goal`, `/config`, and `/subtask`. |
| TUI-local commands | Keep `/help` and `/reload:skills` handled by the TUI. |
| Unknown-command guidance | Use the same command metadata source for available-command text. |

## Core Features

| # | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | Slash Command Suggestions | Critical | Show matching command names and short descriptions when the user types `/` in a safe top-level command position. |
| F2 | Keyboard Navigation | Critical | Support Up/Down selection plus Tab and Enter acceptance, while preserving raw input until acceptance. |
| F3 | Narrow Command Metadata | Critical | Define a small shared metadata source for V1 commands and prompt prefixes used by dropdown/help/error guidance. |
| F4 | Fixed V1 Command Set | Critical | Include only `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, and `/reload:skills`. |
| F5 | State-Aware Activation | Critical | Do not activate during clarification or approval input; avoid interfering with literal slash-prefixed answers. |
| F6 | Prefix Precedence | High | Preserve existing `/agent:` and `/skill:` suggestion flows once those prefixes are active. |
| F7 | Focused Test Coverage | High | Cover render, filter, navigation, accept, dismissal, prefix precedence, and no activation during waiting states. |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Command visibility | 100% of the fixed V1 command set visible from `/` | Unit test command metadata against the approved command list. |
| Unknown-command reduction | -60% unknown slash-command submissions | Compare session events before/after feature usage. |
| Selection efficiency | <= 3 keystrokes after `/` for prefix-matched common commands | Interaction tests for `/h`, `/c`, `/g`, and `/s` flows. |
| Keyboard reliability | 100% coverage for show/filter/Up/Down/Tab/Enter/Escape | Focused TUI tests for each interaction. |
| Metadata drift | 0 duplicated command-description lists for V1 commands | Static/code review check that dropdown/help/error consume shared metadata. |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Strong with state-aware metadata |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: Quick Win

## Council Insights

- **Recommended approach:** Ship the V1 dropdown as a thin discoverability layer over narrow command metadata, not as a full command platform.
- **Key trade-offs:** A local hardcoded list is fastest but worsens drift; a full palette is richer but too broad for V1.
- **Risks identified:** Input theft, stale metadata, prefix conflicts, and confusion during clarification answers.
- **Stretch goal (V2+):** Add richer command palette behavior with categories, disabled reasons, aliases, recent ranking, and user/project custom commands.

## Out of Scope (V1)

- **Configurable custom commands** - Valuable later, but expands V1 beyond command discovery.
- **Full fuzzy command palette** - Adds ranking and mode complexity before basic `/` discovery is validated.
- **Disabled reasons and permission badges** - Useful for safety-aware UX, but can follow after the metadata seam exists.
- **Recent-command ranking** - Requires history behavior and ranking rules outside the MVP.
- **Executing commands directly from the dropdown** - V1 should insert/complete command text only and preserve existing submission paths.

## Architecture Decision Records

- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) - Proceed with a state-aware discovery dropdown backed by narrow shared command metadata.

## Open Questions

None for V1.

Resolved during review:

- V1 includes only `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, and `/reload:skills`.
- Accepting a suggestion inserts command text only and does not submit the prompt.
- `Tab` and `Enter` both accept the selected suggestion.
- The dropdown should reuse the same approach and UI as the existing skill and agent dropdowns.

## References

- Claude Code interactive mode: <https://code.claude.com/docs/en/interactive-mode>
- Cursor slash commands changelog: <https://cursor.com/changelog/1-6>
- Slack shortcuts: <https://slack.com/help/articles/201259356-Slash-commands-in-Slack>
- W3C combobox pattern: <https://www.w3.org/WAI/ARIA/apg/patterns/combobox/>
- Stack Overflow 2025 Developer Survey: <https://survey.stackoverflow.co/2025/>
