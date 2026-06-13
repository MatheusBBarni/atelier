# PRD: @-Mention File Dropdown

## Overview

Atelier routes user prompts to coding agents that operate on a real repository, so nearly every substantive prompt names a file. Today users type those paths by hand, which produces mistyped paths that resolve to nothing and half-remembered paths users guess wrong — each wasting an agent step and eroding trust in the run.

This feature adds an `@`-triggered file/folder picker to the TUI composer. Typing `@` anywhere in a prompt opens a dropdown that fuzzy-searches the project's files and folders; selecting one inserts its path as plain text. It is for **anyone writing prompts in Atelier** — from the daily driver referencing several files a session to the newcomer who doesn't know the tree — and it makes file referencing **accurate by construction** (only real paths are offered) and **fast** (a few keystrokes to a deep file). It mirrors the discovery model Atelier already ships for `/`, `/agent:`, and `/skill:`.

## Goals

- **Eliminate broken file references.** Make it effortless to reference a real, existing path so prompts stop carrying typo'd or non-existent paths.
- **Make referencing faster than typing.** Let a user reach any project file in a handful of keystrokes, regardless of nesting depth.
- **Meet the now-standard expectation.** `@`-file-mentions are table stakes in AI coding tools; deliver the experience users arrive expecting, in a pure terminal.
- **Stay safe and deterministic.** Offer only in-project, non-ignored, non-secret paths; keep selection a text insert so the harness's existing permission checks remain the boundary for any real file read.
- **Timeline:** single release (one milestone), reusing the existing dropdown machinery.

## User Stories

**Primary persona — the daily driver**
- As a developer using Atelier daily, I want to type `@` and fuzzy-find a file so I can reference it accurately in a few keystrokes without recalling its exact path.
- As a developer, I want to reference several files in one prompt so I can ask for cross-file work, by typing another `@` after each one.
- As a developer, I want to reference a whole folder (e.g. `src/tui/`) so I can scope a request to a directory.

**Secondary persona — the newcomer / occasional user**
- As a new user, I want typing `@` to reveal my project's files so I can discover and reference them without knowing the tree.
- As a returning user, I want the picker to surface what I've recently touched so my common references are one keystroke away.

**Edge cases**
- As a user answering a clarification or approval prompt, I want a literal `@` in my answer to stay normal text so the picker doesn't hijack my response.
- As a user in a large repository, I want the picker to stay instant and never block my typing.

## Core Features

| # | Capability | Priority | What the user gets |
| --- | --- | --- | --- |
| F1 | `@` file/folder suggestions | Critical | Typing `@` anywhere in the composer opens a dropdown of project files and folders for the token at the cursor. |
| F2 | Fuzzy match + highlight | Critical | Typing a fragment fuzzy-matches paths; the matched characters are highlighted in each row so the right file is confirmable at a glance. |
| F3 | Relevance ranking | Critical | The most likely file ranks first — recently-edited and shallower paths surface above deep/old ones. |
| F4 | Relevant candidates only | Critical | Results exclude files ignored via `.gitignore`, build/dependency noise, and known secret files; only paths inside the project appear. |
| F5 | Bare-path insert + multi-reference | Critical | Accepting inserts the bare path (folders end with `/`), preserves surrounding text, and leaves the cursor ready to keep typing — so multiple `@` references in one prompt just work. |
| F6 | Keyboard control + empty/no-match states | Critical | Up/Down to select, Tab or Enter to accept, Esc to dismiss; a bare `@` shows recent files; zero matches shows an explicit "No matching files" row. |
| F7 | State-aware activation | High | The picker stays out of the way during approval and clarification prompts, so literal `@` answers remain normal input. |
| F8 | Text-only selection | High | Selecting inserts text only — it never reads file contents — so the harness capability sandbox still governs any actual read. |

## User Experience

**First contact.** A user typing a prompt presses `@`. A compact dropdown appears above the composer showing their most recently-edited / shallow files, the first one highlighted — the same look and keys as the `/` command and `/agent:` dropdowns they already use.

**Typical flow.** The user types a fragment of a name or path (`tuimod`, `claude`). The list narrows to ranked fuzzy matches with the matched characters highlighted. They press Up/Down if needed, then **Tab or Enter**; the `@fragment` is replaced in place by the bare path, a trailing space is added, and the cursor lands right after it. The rest of their sentence is untouched. They continue typing — and can type another `@` to add a second file.

**Folders.** Selecting a folder inserts its path with a trailing `/` and closes the dropdown, exactly like a file; the user can reference a directory as easily as a file.

**Edge and empty states.** If nothing matches, the dropdown shows a single "No matching files" line and does not silently vanish, so `@` never feels broken. **Esc** dismisses the dropdown without altering the typed text, and it stays dismissed until the user edits the token. During approval/clarification prompts the dropdown does not appear, so an answer like `@reviewer, yes` stays literal.

**Discoverability & accessibility.** Discovery is free: users already know the `/` dropdowns, and `@` behaves identically. It is fully keyboard-driven (no mouse), works in any terminal, and relies on selection highlight + matched-character emphasis rather than color alone. The README's "TUI commands" section will note the `@` picker alongside the existing dropdowns.

## High-Level Technical Constraints

- **Performance (user-perceived):** the dropdown appears and filters with no perceptible lag (target < 150 ms) even on large repositories, and never blocks or stutters typing.
- **Privacy & safety:** only files within the project working directory are offered; `.gitignore`-ignored files and known secret files (e.g. `.env`, private keys) never appear; selection inserts text only and does not read file contents.
- **Consistency:** must coexist with the existing `/`, `/agent:`, and `/skill:` dropdowns and their activation precedence, and follow the same keyboard and visual conventions.

## Non-Goals (Out of Scope)

- **File-content attachment** — V1 inserts a path only; it does not load file contents into the run. (Strong V2 candidate.)
- **Symbol or line references** (`@symbol`, `@file:42`) — reserved as the strongest later differentiation bet.
- **In-dropdown file preview / browsing** — no content preview; this would change the safety posture.
- **References outside the project working directory** — configured extra read-roots are not searched in V1.
- **Folder drill-in / navigation mode** — folders are referenceable values, not a browse tree.
- **Cross-session recent-files memory** — "recent" is derived per session, not persisted history.

## Phased Rollout Plan

### MVP (Phase 1) — this PRD
- All Core Features F1–F8 shipped as one release (Approach A).
- **Proceed criteria:** accepted suggestions resolve to real paths ~100% of the time; broken file-path occurrences trend toward the −70% target; the picker stays within the latency target on a large repo; users adopt `@` for file referencing.

### Phase 2 — Symbol & codemap references
- Extend `@` to reference functions/types/codemap entries, not just files.
- **Proceed criteria:** V1 adoption is healthy and users request code-level references.

### Phase 3 — Content attachment (and scale)
- Optionally let a reference attach file contents to the run, behind the same selection seam; add a maintained index only if very large repositories demand it.
- **Long-term success:** `@` becomes the default way users bring file context into a prompt across runtimes.

## Success Metrics

| Metric | Target | How measured |
| --- | --- | --- |
| Path resolution accuracy | ~100% of accepted suggestions resolve to a real path | Verify every accepted candidate exists under the project root |
| Broken-path reduction | −70% prompts containing non-resolving file tokens | Compare session prompt history before/after |
| Keystroke efficiency | ≤ 6 keystrokes after `@` to rank a deep file in the top 3 | Interaction tests on representative deep paths |
| Responsiveness | < 150 ms to first results on a 10k-file repo | Benchmark on a large fixture; no input lag |
| Noise exclusion | 0 ignored, secret, or out-of-project entries shown | Fixture repo with ignored/secret files |
| Adoption | Majority of file-referencing prompts use `@` | Share of prompts where a path entered via the picker |

## Risks and Mitigations

- **Low differentiation (table stakes).** The feature matches competitors rather than setting Atelier apart. *Mitigation:* win on execution (accuracy, speed) and lean on the agent-agnostic angle — an inserted path works no matter which runtime the prompt routes to.
- **Adoption inertia.** Users keep typing paths by hand. *Mitigation:* mirror the familiar `/` dropdown exactly (zero learning curve) and surface it in the README/onboarding.
- **Trust/safety perception.** Surfacing a sensitive filename would undermine confidence. *Mitigation:* `.gitignore` + secret-name exclusion + project-only scope are product guarantees, covered by tests.
- **Noise on very large repositories.** Too many matches overwhelm the list. *Mitigation:* cap visible results and show a "refine your query" affordance.
- **Single-release scope.** A complete V1 is a larger first landing than a thin slice. *Mitigation:* heavy reuse of existing dropdown machinery keeps the surface small.

## Architecture Decision Records

- [ADR-001: Scope @-Mention File Dropdown V1](adrs/adr-001.md) — Fuzzy `@` dropdown over a lazy `.gitignore`-aware walk, bare-path insert through a structured reference seam, with security guardrails.
- [ADR-002: Package as a Complete Single-Release V1](adrs/adr-002.md) — Ship the full experience at once rather than phasing thin→thick or expanding it.

## Open Questions

- **Path escaping:** how a selected path containing spaces or shell-special characters should appear in the prompt text (raw, quoted, or escaped) — to be settled in the techspec.
- **"Recent" definition:** whether the empty-`@` recents are driven by this-session edits, on-disk modification time, or both — a UX detail to tune.
- **Result cap & "too many" threshold:** the maximum rows shown and when to prompt the user to refine.
- **Non-git directories:** expected behavior when the working directory is not a git repository (no `.gitignore` to honor).
