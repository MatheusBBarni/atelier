# @-Mention File Dropdown

## Overview

Add an `@`-triggered file/folder dropdown to Atelier's TUI composer. When a user types `@` anywhere in a prompt, a dropdown shows matching repository files and folders; selecting one inserts its path as plain text. V1 optimizes for **accurate, fast file referencing** — users stop hand-typing (and mistyping) paths, and reach deep files in a few keystrokes via fuzzy matching.

The MVP fuzzy-matches against a `.gitignore`-respecting walk of the working directory, ranks the most likely file to the top, highlights matched characters, and inserts the bare path on `Tab`/`Enter`. It reuses the existing dropdown machinery (the same patterns that power `/`, `/agent:`, and `/skill:`) and keeps selection text-only, so the harness capability sandbox remains the boundary for any actual file reads. Ambition: a **Quick Win** that ships a table-stakes capability cheaply while leaving a clean seam to grow into content-attachment and richer references later.

## Problem

Atelier routes prompts to coding agents that operate on a real repository, so almost every substantive prompt names a file: *"refactor `src/runtime/claude.rs`"*, *"compare the two dropdown renderers"*. Today the user types those paths by hand. In a deep Rust tree (`src/tui/mod.rs`, `src/runtime/...`, `.compozy/tasks/...`) that means recalling exact spelling and nesting, which produces two failure modes: **typos that resolve to nothing**, and **half-remembered paths the user abandons or guesses wrong**. A broken path wastes an agent step (a failed read) and erodes trust in the run.

The TUI already proves that dropdown completion works — for `/` commands, `/agent:`, and `/skill:` — and the `/agent:`/`/skill:` variants already trigger **mid-prompt**. The missing piece is file references, the single most common thing a coding prompt points at. Closing that gap turns "remember and type the path" into "type a fragment, pick the file," which is accurate *by construction* because the dropdown only ever offers paths that exist.

### Market Data

`@`-file-mentions are now standard in AI coding tools and explicitly described as "table stakes." OpenAI's **Codex CLI** is the exact analogue of this design: typing `@` opens a fuzzy file search over the workspace and `Tab`/`Enter` drops the **path** into the message. Most IDE peers — Claude Code, Cursor, Continue.dev, GitHub Copilot Chat — instead attach file *content* (heavier, and coupled to one provider's context machinery). Adoption context: ~80% of developers report using AI tools (2025 Stack Overflow Developer Survey), while trust in AI accuracy is low (~29%) — which rewards **explicit, deterministic, inspectable** context control (a path the developer chose) over opaque semantic retrieval.

## Summary / Differentiator

The differentiator is not autocomplete itself — it's that a **path is agent-agnostic**. Because selection inserts a path string (not provider-specific attached content), the same `@`-mention behaves identically no matter which runtime (`codex`, `claude`, `cursor`, `zai`) the prompt routes to. Paired with `.gitignore`-correct candidates (via ripgrep's `ignore` walker) and fzf-grade fuzzy matching (via `nucleo`), Atelier brings the IDE-grade `@` experience to a pure-terminal, multi-runtime harness while keeping context selection deterministic.

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| TUI composer | Show a dropdown when `@` starts a token at the cursor, anywhere in the input. |
| `/agent:` & `/skill:` dropdowns | Reuse the mid-prompt token detector, substring/token-replacement pattern, and precedence routing; `@` slots into the same routing order. |
| `/` command dropdown | Reuse render, keyboard handling, no-match state, and cursor/byte insertion helpers. |
| Codemap walker (`src/codemap`) | Sibling gitignore-aware walk; codemap excludes by a hard-coded list (not `.gitignore`), so the new walk uses the `ignore` crate rather than reusing that traversal. |
| Capability sandbox | Selection stays string-only; the downstream agent's actual file read is still gated by read capability and read-roots. |

## Core Features

| # | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | `@` File/Folder Suggestions | Critical | Typing `@` anywhere in the composer opens a dropdown of repo files and folders for the token at the cursor. |
| F2 | Fuzzy Matching + Highlighting | Critical | Match the typed fragment against candidate paths with `nucleo` subsequence matching; highlight the matched characters in each row. |
| F3 | Relevance Ranking | Critical | Rank by `nucleo` score with shallow-path and most-recently-edited boosts, alphabetical as final tiebreak, so the intended file lands near the top. |
| F4 | Gitignore-Aware Candidate Walk | Critical | Build candidates from an `ignore`-crate walk pinned to the working directory; lazy on first `@`, cached per dropdown, refreshed on re-open — no persistent index or watcher. |
| F5 | Security Guardrails | Critical | Static secret-name denylist (`.env*`, `*.pem`, `*.key`, `id_rsa*`, `.ssh/`, `.aws/`), no symlink-follow, reject candidates resolving outside the root, keep `.git/` excluded, never union `extra_read_roots`, string-only on select. |
| F6 | Keyboard Nav + Bare-Path Insert | Critical | Up/Down select, Tab/Enter accept, Esc dismiss; insert the bare path (folders get a trailing `/`) via one `insert_selection(Candidate)`; compact no-match state; disabled during clarification/approval. |
| F7 | Reference Seam | High | Capture selection as a structured `Candidate` through a single insert join point, with the walk/match in a non-TUI `FileIndex` — keeps V2 attachment and a watched index swap-in without rework. |
| F8 | Focused Test Coverage | High | Cover render, fuzzy filter, ranking, navigation, accept, dismiss, mid-prompt activation, no-activation during waiting states, and security cases (denylist, symlink/`..` rejection, gitignore exclusion). |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Path resolution accuracy | 100% of accepted suggestions resolve to a real path | Test that every accepted candidate is an existing entry under the root |
| Broken-path reduction | -70% prompts containing non-resolving file tokens | Compare session events (path-like tokens that don't resolve) before/after |
| Keystroke efficiency | ≤ 6 keystrokes after `@` to rank a deep file in the top 3 | Interaction tests on representative deep paths (e.g. `src/runtime/claude.rs`) |
| First-result latency | < 150 ms on a 10k-file repo | Benchmark walk + match on a large fixture, off the draw thread |
| Noise exclusion | 0 git-ignored, denylisted, or out-of-root entries in results | Fixture repo with `node_modules`/`target`/`.env` + `.gitignore` |
| Keyboard reliability | 100% coverage: show/filter/Up/Down/Tab/Enter/Esc + mid-prompt | Focused TUI tests for each interaction |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Maybe — table stakes; mild edge from agent-agnostic path insert |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe — standard UX, low moat |
| **Feasibility** | Can we actually build this? | Strong — ~90% reuse + two proven crates |

Leverage type: **Quick Win**

## Council Insights

- **Recommended approach:** Ship a thin discoverability layer over the existing dropdown machinery; keep selection string-only so the capability sandbox stays the read boundary; capture the validated selection as a structured `Candidate` through one insert join point.
- **Key trade-offs:** True fuzzy (`nucleo`) vs. substring-only — the council majority favored substring-first; the product owner chose fuzzy-in-V1 for the power-user retrieval gesture, accepting one extra crate. Persistent watched index vs. lazy walk — resolved to a **lazy non-persistent walk** (fuzzy needs no index), avoiding the staleness lifecycle. Abstraction — a single `insert_selection(Candidate)` and a concrete non-TUI `FileIndex`, **not** a speculative provider trait.
- **Risks identified:** (1) `.gitignore` filters tracked-ness, not sensitivity → untracked-but-unignored secrets could surface by name → denylist + root pin + no symlinks. (2) Large-repo first-`@` walk latency → off-thread/lazy walk + result cap. (3) Within-session staleness → refresh on re-open.
- **Dissenting view (recorded):** product-mind holds that for an fzf/Telescope-trained audience, true subsequence fuzzy is the expected gesture — which is the version chosen.
- **Stretch goal (V2+):** `@`-symbol/codemap references (functions, types) — strongest differentiation bet; and content-attachment, both built behind the V1 seam.

## Out of Scope (V1)

- **Persistent / watched file index** — the lazy non-persistent walk avoids index staleness; a watcher is deferred until a proven large-monorepo need.
- **Content attachment (loading file contents)** — keeps the capability sandbox as the read boundary; deferred behind the insert seam.
- **Line-range / symbol references (`@file:42`, `@symbol`)** — needs AST/symbol indexing; reserved as the strongest V2+ differentiation bet.
- **Unioning `extra_read_roots` into candidates** — fuzzy-searching read-roots risks discovery over sensitive trees; V1 pins to the working directory.
- **Preview-on-hover / content preview** — would turn the walker into a read primitive and raise the sandbox bar; defer.
- **`FileIndexProvider` trait / mention registry / typed reference token** — premature with one implementation; extract from `FileIndex` when a second appears.

## Architecture Decision Records

- [ADR-001: Scope @-Mention File Dropdown V1](adrs/adr-001.md) — Ship a state-aware `@` dropdown with `nucleo` fuzzy over a lazy `.gitignore`-aware walk, bare-path insert through a structured `Candidate` seam, with security guardrails.

## Open Questions

- **Path escaping:** how should inserted paths with spaces or shell-special characters be rendered (quote, backslash-escape, or raw)? Decide in the techspec.
- **Ranking weights:** the exact blend of `nucleo` score vs. shallow-path vs. recently-edited boosts — tune during implementation against a real repo.
- **Result cap + "refine" threshold:** the maximum rows shown and when to show the "too many matches" hint — tune empirically.
- **Non-git directories:** when the working directory isn't a git repo (no `.gitignore`), the `ignore` walk falls back to default excludes only — confirm that's acceptable.

## References

- Slash command dropdown artifacts: `.compozy/tasks/slash-command-dropdown/`
- Codex CLI `@` fuzzy file search (insert path): <https://developers.openai.com/codex/cli/features>
- `ignore` crate (ripgrep walker): <https://docs.rs/ignore>
- `nucleo` fuzzy matcher: <https://github.com/helix-editor/nucleo>
- W3C combobox pattern: <https://www.w3.org/WAI/ARIA/apg/patterns/combobox/>
- Stack Overflow 2025 Developer Survey (AI): <https://survey.stackoverflow.co/2025/ai>
