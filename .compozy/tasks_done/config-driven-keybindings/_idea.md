# Idea: Config-Driven Keybindings

## Overview

Let atelier users remap the TUI's keyboard shortcuts through an optional `[keybindings]`
config section, instead of living with bindings hardcoded in the key-routing functions. V1
targets two users: the **muscle-memory power user** (vim/emacs/readline reflexes) and the
**terminal/OS-conflict victim** (a key atelier uses, classically `Ctrl-L`, is swallowed or
claimed by their terminal/multiplexer). The feature ships a keymap consulted before the
hardcoded routing cascade, a forward-compatible **context-scoped** schema (only the `normal`
context is wired in V1), a structurally non-rebindable safety set, **new readline/emacs
line-editing actions in the composer**, and a live help tab that renders the active keymap.

V1 is a **Quick Win with a compounding foundation**: the routing functions are already pure
`(state, KeyEvent) → Option<TuiCommand>`, so the override is a lookup-before-fallback with
zero change to default behavior — and the context-scoped schema makes the ambitious V2
(modal-key remapping, `vim`/`emacs` presets, leader-key) additive rather than a format
migration.

## Problem

atelier's every keybinding is hardcoded inside `key_event_to_tui_command_with_ui`
(`src/tui/mod.rs:1056`) and `key_event_to_tui_command` (`1388`). A user whose terminal, tmux,
or screen already claims `Ctrl-L` cannot reach the roster toggle at all — the key is simply
broken for them, a hard adoption blocker hit in the first session, with **no workaround**. A
vim/emacs/readline user faces constant low-grade friction: the composer doesn't even support
`Ctrl-A`/`Ctrl-E`/`Ctrl-K` line editing (those actions don't exist yet), so decades of muscle
memory misfire on every prompt. Accessibility users who struggle with chords have no way to
choose comfortable bindings.

Remapping is also an **explicit charter item** and a recurring TUI-adoption blocker. The cost
of inaction compounds: the hardcoded table cannot adapt to new terminals, conflicts, or input
preferences, and the static help text silently drifts from the real bindings.

### Market Data

- Leading TUIs converged on configurable, **mode/context-scoped** keymaps with merge-over-
  defaults: **helix** (`[keys.normal]`/`[keys.insert]`), **zellij** (KDL `keybinds`), **yazi**
  (`keymap.toml`), **atuin** (per-mode tables). An unbind sentinel is standard. *(official docs)*
- Among AI agent CLIs, **Claude Code** is the closest analog: `~/.claude/keybindings.json`,
  ~20 contexts, `namespace:action` syntax, a **hard-reserved safety set** (`Ctrl+C/D/M`,
  Caps Lock), and **load-time validation** (duplicate/reserved/multiplexer conflicts) via
  `/doctor`. **Codex CLI** shipped a vim mode in response to user issues; **k9s** *refused*
  remapping (issue #1391, closed not-planned) and took user heat — a visible unmet-demand
  signal. *(official docs + GitHub issues)*
- **Terminal limits** constrain what is bindable: in legacy mode `Tab==Ctrl+I`, `Enter==Ctrl+M`,
  `Esc==Ctrl+[` emit identical bytes; `Ctrl+digit`/`Ctrl+Shift+*`/`Cmd` need the **Kitty
  keyboard protocol** (partial terminal support). `Ctrl-L` is bindable but conventionally
  claimed — best solved by letting users remap *away* from it. *(kitty/crossterm docs)*
- **Accessibility** has a standards hook: **WCAG 2.1.4 Character Key Shortcuts** requires
  single-key shortcuts be disable-able or remappable. *(W3C)*

## Summary / Differentiator

atelier's modal precedence cascade (help → clarification → approval → dropdowns → queue →
normal) is *already a context taxonomy*, so a context-scoped keymap is native rather than
bolted on. And because approval prompts gate `WriteFile`/`RunCommand`, atelier's fixed
safety set is a **correctness guarantee, not cosmetic convention** — a stronger position than
helix's or lazygit's "we just don't let you." Loading keybindings from **user scope only**
(never a cloned repo's `./atelier.toml`) means a hostile project can't reconfigure the keys
you use to *say no* to an agent.

## Core Features

| #  | Feature | Priority | Description |
| -- | ------- | -------- | ----------- |
| F1 | Keymap override engine + context-scoped schema | Critical | A keymap consulted at the top of normal-input routing, before the hardcoded cascade. Absent config ⇒ byte-for-byte identical defaults. On-disk format is context-scoped (`[keybindings.normal]`); only `normal` is wired in V1, so V2 contexts are additive. |
| F2 | Reserved safety set via single pre-lookup chokepoint | Critical | Consolidate the three `Ctrl-C` branches into one guard that returns quit/interrupt *before* any keymap lookup, making them structurally non-rebindable. A config that tries to bind a reserved key is hard-rejected. Approval accept/reject keys stay fixed. |
| F3 | Composer line-editing actions (Hybrid) | Critical | New normal-mode actions in the input composer: `Ctrl-A` (line start), `Ctrl-E` (line end), `Ctrl-K` (kill-to-end), `Ctrl-U` (kill line), `Ctrl-W` (kill word) — serving readline/emacs muscle memory, remappable like any binding, no modal-handler changes. |
| F4 | User-scope loading + portable-allowlist validation | High | `[keybindings]` loads from home/global config + CLI only, **not** the local/project `./atelier.toml`. Bindable keys restricted to a portable allowlist (Ctrl+letter except `C/D/I/M/[`, function keys, arrows, `Page/Home/End`); non-portable keys rejected at load. |
| F5 | Validation posture + diagnostics | High | **Hard-fail** security/structural violations (reserved-key bind, unknown context, malformed); **fail-soft** cosmetic ones (unknown action, unsupported key) with per-entry default fallback. All surfaced via `atelier --doctor`; `--print-config` emits the effective keymap. |
| F6 | Live Keys help tab | High | The existing static `Keys` tab (`keys_tab_lines`, `src/tui/mod.rs:3737`) becomes data-driven, rendering the active keymap with fixed/reserved keys visibly locked. Colors via theme tokens only. |
| F7 | Action-name ↔ `TuiCommand` integrity | Medium | An exhaustive compile-checked match binds action names to `TuiCommand` variants (no drift), plus a default-fidelity regression test and a parse round-trip test. |

## KPIs

| KPI | Target | How to Measure |
| --- | ------ | -------------- |
| Default-binding fidelity | **100% byte-for-byte** | Contract test: no `[keybindings]` ⇒ identical `(KeyEvent→TuiCommand)` set as today's table |
| Security/structural rejection | **100%** of reserved-key binds, unknown contexts, non-portable keys rejected at load with file+field+value error | Unit tests over the validator; `--doctor` non-zero exit |
| Keys-tab drift | **0** (100% derived) | Test asserts the Keys tab renders from active keymap data, not literals |
| Remap correctness | **100%** of remappable normal-mode actions (incl. the 5 line-editing) reachable via a remapped key | Round-trip/property test: parse → `KeyEvent` → lookup → expected `TuiCommand` |
| Composer editing coverage | **≥ 5** readline actions present, buffer-correct, and bindable | Test per action: exists, mutates buffer correctly, in the bindable set |
| Time-to-first-rebind | **< 2 min** from docs | Doc walkthrough / moderated test: edit user config → restart → verify in live Keys tab |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | **Strong** — removes a hard blocker + serves readline/emacs editing |
| **Reach** | What % of users would this affect? | **Strong** — composer line-editing benefits anyone typing prompts; remap helps the conflicted minority |
| **Frequency** | How often would users encounter this value? | **Strong** — set once, felt every keystroke; live Keys tab helps all |
| **Differentiation** | Does this set us apart or just match competitors? | **Strong** — few open agent harnesses have it; safety-as-correctness + user-scope trust is unique |
| **Defensibility** | Easy to copy or compounds? | **Maybe** — table stakes once present; the context-scoped foundation + doctor diagnostics compound |
| **Feasibility** | Can we actually build this? | **Strong** — pure seam, config precedent, Keys tab + doctor exist; line-editing adds modest logic |

Leverage type: **Quick Win** (with a compounding foundation for V2 presets/contexts).

## Council Insights

- **Recommended approach:** Ship the keymap engine + a forward-compatible, context-scoped TOML
  schema (`[keybindings.normal]`), wiring only the `normal` context, **plus** the composer
  line-editing slice (the part of the ambitious vision that fits the normal-mode charter).
  Override is consult-before-fallback (routing core untouched). Safety enforced structurally
  via a single pre-lookup chokepoint. Keybindings load from user scope only. Validation
  hard-fails the security-critical and fail-softs the cosmetic, surfaced through `--doctor`.
- **Key trade-offs:** context-scoped schema now vs flat-and-refactor (resolved: schema is the
  irreversible contract — scope it now); presets/leader-key now vs later (resolved: they need
  the action vocabulary + modal-handler changes → V2, additive over this schema); fail-fast vs
  fail-soft (resolved: split by severity); ladder uniformity vs trust (resolved: keybindings
  are user-scope, a different trust class than project settings).
- **Risks identified:** audience mismatch — `hjkl` *navigation* users only partially served in
  V1 (→ honest framing + V2 path); action-name↔`TuiCommand` drift (→ exhaustive match +
  round-trip test); existing `keys_tab_lines_contains_expected_keybindings` test asserts
  hardcoded strings (→ rewrite to "no config ⇒ byte-identical"); reserved-set must dominate all
  three `Ctrl-C` branches (→ consolidate to one chokepoint); a user binding a terminal-
  undeliverable key (→ designed out by the portable allowlist).
- **Stretch goal (V2+):** modal/dropdown-key remapping over the same context schema, `hjkl`
  list-navigation, shipped `vim`/`emacs` presets (reusing the `[presets.*]` mental model), an
  OpenCode-style leader-key escape hatch, Kitty-protocol detection for richer chords, an
  interactive in-app rebinding UI, and hot-reload.

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| Key routing (`key_event_to_tui_command*`, `src/tui/mod.rs:1056`/`1388`) | Keymap consulted before the normal-input cascade; three `Ctrl-C` branches consolidated to one pre-lookup guard |
| Config ladder (`apply_raw:909`, `load_effective_config:1647`) | New `[keybindings]` section via the existing merge — but applied only from home/global + CLI layers |
| Help `Keys` tab (`keys_tab_lines`, `src/tui/mod.rs:3737`) | Becomes data-driven from the active keymap; shipped by the `help-modal-tabs` feature |
| `atelier --doctor` (`src/doctor/mod.rs`) + `--print-config` | Surface validation diagnostics and the effective keymap |
| Composer input (`InputCharacter`/`InputBackspace`/`MoveInputCursor`) | New line-editing `TuiCommand`s + buffer logic |
| `TuiCommand` enum (`src/tui/mod.rs:169`) | New variants; exhaustive match maps names ↔ variants |
| `theme.rs` | Keys-tab styling via semantic tokens (`colors_live_only_in_theme_module`) |

## Out of Scope (V1)

- **Remapping modal/dropdown keys** (approval, clarification, help, all dropdowns) — fixed in
  V1; approval accept/reject is a security control surface. Deferred to V2 with the schema
  already context-ready.
- **`hjkl` list-navigation remapping** — lives in the fixed modal/dropdown handlers; lifting it
  out touches the well-tested routing core. V2.
- **Shipped `vim`/`emacs` presets & leader-key** — need the full action vocabulary / a new
  modal input state; become additive surfaces over this schema in V2.
- **Honoring `[keybindings]` from local/project `./atelier.toml`** — untrusted project config;
  user-scope only in V1 (removes the malicious-project-config vector).
- **Kitty keyboard protocol detection / `Ctrl+digit` & `Ctrl+Shift+*` chords** — partial
  terminal support + capability branching; the portable allowlist covers target users. V2.
- **Interactive in-app rebinding UI and hot-reload** — V2+; both build on the same config.

## Architecture Decision Records

- [ADR-001: V1 Scope for Config-Driven Keybindings](adrs/adr-001.md) — Hybrid of "Foundation +
  conflict relief" plus composer line-editing: context-scoped schema (normal-only wired),
  portable-allowlist validation, single Ctrl-C safety chokepoint, user-scope loading, and a
  severity-split validation posture.

## Open Questions

1. **Validation posture (confirmed):** hard-fail on security/structural violations, fail-soft
   (default fallback + collected warnings via `--doctor`) on cosmetic ones — confirmed at draft
   review over strict fail-fast everywhere.
2. **Key-string syntax:** one canonical format for the config — e.g. `"ctrl+l"` vs `"C-l"` vs
   `"Ctrl-L"`. Which reads best for atelier's audience? (Decide in TechSpec.)
3. **Line-editing set:** confirm `Ctrl-A/E/K/U/W` for V1 (add `Ctrl-B/F` char-move?
   `Alt-Backspace`?).
4. **Unbind sentinel:** support disabling a default binding (e.g. `"none"`/`<disabled>`) in V1,
   or defer?
5. **Keys-tab detail:** basic live view confirmed for V1; the "customized vs default" marker
   stays V2.
