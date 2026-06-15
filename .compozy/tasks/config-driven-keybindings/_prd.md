# PRD: Config-Driven Keybindings

## Overview

atelier's keybindings are hardcoded in the TUI's key-routing functions, and the input composer
lacks the readline line-editing (`Ctrl-A/E/K/U/W`) that every shell and REPL ships — so the
text input "feels broken" to anyone with terminal muscle memory, and any user whose terminal or
multiplexer claims a key atelier uses (classically `Ctrl-L`) has that capability permanently
broken with no recourse.

This feature makes the composer **obey terminal/editor muscle memory by default** and lets users
**remap or disable** atelier's normal-mode keys through an optional, user-scope `[keybindings]`
config section, with the active map always visible in the help modal. The primary outcome we
optimize for is **native input feel + recovering keys the user's terminal claims** — a broad,
adoption-agnostic win. Power-user customization and WCAG 2.1.4 accessibility are deliberate
secondary benefits. Delivery is **parity-first**: better defaults for everyone first, the
customization layer second.

## Goals

- **Primary:** The composer obeys readline/editor muscle memory out of the box (`Ctrl-A/E/K/U/W`),
  so atelier feels native to terminal users with no configuration.
- **Primary:** No atelier key stays permanently broken — a user can **remap or unbind** any
  conflicting normal-mode key so their terminal/multiplexer can have it back.
- Let power users customize normal-mode bindings **safely and opt-in**, never weakening the
  interrupt/quit kill-switch or the approval controls that gate writes.
- Keep the **active keymap self-evident in-app** (live Keys tab) and documented in the config
  authoring surfaces.
- Provide an **accessibility** remap path consistent with WCAG 2.1.4.
- **Milestones:** Wave 1 (line-editing defaults + safety chokepoint + data-driven Keys tab) →
  Wave 2 (remap engine + user-scope config + unbind + `--doctor` validation) → V2 (presets,
  leader-key, modal-context remapping).

## User Stories

**Priya — power user (primary; readline/vim muscle memory):**
- As a shell power user, I want `Ctrl-A/E/K/U/W` to edit my prompt the way they do in bash, so I
  don't fight the input on every prompt.
- As a vim/emacs user, I want to remap atelier's normal-mode keys to my preferred layout, so the
  tool matches my muscle memory.
- As a customizer, I want to see my active bindings in-app, so I can confirm a remap took effect.

**Cole — terminal-conflict victim (primary; `Ctrl-L` is swallowed by tmux/terminal):**
- As a user whose terminal eats `Ctrl-L`, I want to move that action to a free key, so I can use
  the feature it triggers.
- As a tmux user, I want to **disable** an atelier binding entirely, so my multiplexer keeps that
  key.
- As someone editing config, I want a clear error when I mistype a binding, so I can fix it fast.

**Ada — accessibility user (secondary; motor/voice/assistive-tech):**
- As a user who finds certain chords hard to press, I want to rebind core actions to comfortable
  keys, so I can operate atelier without strain.
- As a voice-control user, I want to remap single-key shortcuts that fire accidentally, so the
  app respects assistive-tech needs (WCAG 2.1.4).

**Sam — safety-conscious operator (cross-cutting):**
- As any user, I want interrupt/quit and the approval accept/deny keys to **always** work
  regardless of my config, so I can never lock myself out or weaken write-approval.
- As someone who clones repos, I want a project's config to **never** change my keys, so an
  untrusted repository can't touch my controls.

## Core Features

| Priority | Feature | What it does / why it matters | Wave |
| -------- | ------- | ----------------------------- | ---- |
| Critical | **Composer line-editing defaults** | `Ctrl-A` (line start), `Ctrl-E` (end), `Ctrl-K` (kill-to-end), `Ctrl-U` (kill line), `Ctrl-W` (kill word) in the prompt input. Closes the "feels broken" gap for every user. | 1 |
| Critical | **Reserved safety set (single chokepoint)** | Interrupt/quit resolve *before* any keymap lookup and can never be remapped or unbound; approval accept/deny keys stay fixed. Safety becomes a guarantee, not a convention. | 1 |
| High | **Live Keys help tab** | The Keys tab renders the **active** keymap (Wave 1: defaults; Wave 2: customizations), with reserved/fixed keys shown locked. The in-app discovery surface. | 1 → 2 |
| Critical | **Keymap override engine + config section** | Optional `[keybindings.normal]` consulted before the hardcoded routing; absent config ⇒ identical defaults. Context-scoped format so future contexts are additive. | 2 |
| High | **Unbind a binding** | A user can disable a default binding so their terminal/multiplexer handles the key (reserved keys excepted). Serves the conflict victim directly. | 2 |
| High | **User-scope loading + portable-key validation** | `[keybindings]` is honored only from home/global config + CLI (never a project's `./atelier.toml`); only portable keys are bindable, with non-portable keys rejected at load. | 2 |
| High | **Validation + diagnostics** | Hard-fail security/structural mistakes (reserved-key bind, unknown context, malformed) with precise file+field+value errors; fail-soft cosmetic ones (unknown action, unsupported key) with default fallback — all surfaced via `atelier --doctor`; `--print-config` emits the effective keymap. | 2 |
| Medium | **No-drift integrity** | Defaults stay byte-identical with no config; binding action names cannot drift from the real actions. Protects against silent regressions. | 1 → 2 |

## User Experience

**Priya (power user):** On upgrade, her prompt suddenly respects `Ctrl-A/E/K/U/W` — no setup. To
go further, she reads the `[keybindings]` block in `atelier --init-config` / README, adds a few
lines to her home config, restarts, opens `/help → Keys`, and sees her custom bindings listed
(reserved keys clearly locked).

**Cole (conflict victim):** `Ctrl-L` does nothing because tmux eats it. He opens the Keys tab,
sees `Ctrl-L` is the roster toggle, and either remaps the toggle to a free key or **unbinds**
`Ctrl-L` so tmux keeps it. A typo in his config produces a precise startup/`--doctor` error
pointing at the offending line.

**Ada (accessibility):** She rebinds an awkward chord to a comfortable key in her home config; the
remap path satisfies her need without any special mode.

**UI/UX & accessibility:**
- **Keyboard-first**, monochrome/`NO_COLOR`-legible Keys tab; reserved/fixed keys visibly locked.
- **Discovery is pull, not push:** the live Keys tab + documented config are the surfaces; no
  proactive nudge in V1 (consistent with the established onboarding convention).
- **Fail-soft for cosmetic mistakes** so a typo never bricks the TUI; **hard-fail** only for
  safety/structural violations, reported where the user can act (`--doctor`, startup).
- **Trust:** keybindings come only from the user's own environment, never a cloned repo.

## High-Level Technical Constraints

*(Product boundaries, not implementation prescriptions.)*
- **No-regression default:** with no `[keybindings]` config, behavior is byte-for-byte identical
  to today.
- **Trust boundary:** keybindings are honored only from user scope (home/global config + CLI),
  never the project/local config layer.
- **Non-rebindable safety set:** interrupt/quit and the approval accept/deny keys cannot be
  remapped or unbound in V1.
- **Portable keys only:** bindable keys are limited to a set that works without special terminal
  protocols; non-portable keys are rejected at load (no silent dead keys).
- **No new telemetry / no data leaves the machine:** success is measured by tests and qualitative
  signals, not usage analytics.
- **Keyboard-only operability** and **monochrome legibility** for the Keys tab.

## Non-Goals (Out of Scope)

- **Remapping modal/dropdown keys** (approval, clarification, help, dropdowns) — fixed in V1;
  approval keys are a safety control. Deferred to V2 over the context-scoped schema.
- **`hjkl` list-navigation remapping** — lives in fixed modal handlers; V2.
- **`vim`/`emacs` presets and a leader-key** — need the full action vocabulary / a new input mode;
  V2.
- **Honoring `[keybindings]` from project/local config** — trust boundary; user-scope only.
- **Extended line-editing beyond the core five** (char/word movement, yank, transpose) — deferred;
  the five close the table-stakes gap.
- **Kitty-protocol chords** (`Ctrl+digit`, `Ctrl+Shift+*`, Alt-chords) — portability; V2.
- **Interactive in-app rebinding UI, hot-reload, and proactive discovery nudges** — V2; V1 is
  pull-only via the Keys tab + docs.
- **"Customized vs default" marker in the Keys tab** — V2.

## Phased Rollout Plan

### Wave 1 — MVP (broad parity, no config)
- **Included:** composer line-editing defaults (`Ctrl-A/E/K/U/W`); single interrupt/quit safety
  chokepoint; data-driven Keys tab rendering the default keymap.
- **Success criteria to proceed:** the five editing actions work correctly in the composer;
  defaults are byte-identical to today (no routing regression); the Keys tab renders from keymap
  data (not hardcoded text); interrupt/quit resolve through the single chokepoint.

### Wave 2 — Remap layer
- **Included:** `[keybindings.normal]` override engine (user-scope); portable-key validation;
  unbind; severity-split validation via `atelier --doctor`; Keys tab reflects customizations;
  `--print-config` emits the effective keymap.
- **Success criteria to proceed:** every remappable normal-mode action round-trips from config to
  effect; invalid configs are handled per the severity policy with precise errors; reserved-key
  binds are rejected; project/local `[keybindings]` is ignored; unbind frees a key for the
  terminal.

### Phase 3 / V2
- **Included:** modal/dropdown-context remapping; `vim`/`emacs` presets; leader-key; `hjkl`
  navigation; Kitty-protocol chords; "customized" Keys-tab marker; optional discovery nudge; a
  usage event if telemetry is ever added.
- **Long-term success:** atelier's keymap is fully customizable per context, and bindings are the
  most-discoverable they can be in-app.

## Success Metrics

*(No usage telemetry exists, so metrics are test-based or qualitative — definitions and trends, as
in the help-modal-tabs PRD.)*

| Metric | Definition | Target |
| ------ | ---------- | ------ |
| **Default-binding fidelity** | No `[keybindings]` ⇒ identical key→action behavior as today | **100%** byte-identical (regression test) |
| **Readline parity** | The five line-editing actions exist and edit the buffer correctly | **5/5** present & correct |
| **Remap correctness** | Remappable normal-mode actions reachable via a custom binding | **100%** round-trip (test) |
| **Validation correctness** | Reserved-key binds / non-portable keys / unknown contexts rejected with file+field+value error; cosmetic errors fall back + surface in `--doctor` | **100%** per the severity policy |
| **Trust-boundary enforcement** | Project/local `[keybindings]` never applied | **100%** ignored (test) |
| **Keys-tab accuracy** | Keys tab reflects the active keymap | **0 drift** (renders from data) |
| **Qualitative signal** | Power-user/conflict-victim reports that bindings work; key-conflict support questions | Trend **down**; positive reports |

## Risks and Mitigations

| Risk | Type | Mitigation |
| ---- | ---- | ---------- |
| Few users ever customize (power of defaults) | Adoption | Framed as opt-in; the **defaults** carry the broad value (parity); success is not scored on global adoption |
| Pull-only discovery means some never find remapping | Adoption | Live Keys tab + `--init-config` block + README; revisit a nudge in V2 if discovery proves poor |
| `hjkl`-navigation users only partially served in V1 | Adoption | Honest docs framing; clear V2 path over the same schema |
| Conflict-victim hard-blocker lands in Wave 2, not Wave 1 | Timeline | Keep Wave 2 in the same release cycle; Wave 1 still ships broad parity value |
| An untrusted project config alters a user's keys | Competitive/Trust | Keybindings are user-scope only; reserved + approval keys are fixed |
| Config becomes a contract that's painful to change | Dependency | Context-scoped, version-tolerant, fail-soft schema; unknown entries fall back rather than break |
| Can't prove impact without telemetry | Measurement | Lean on tests + qualitative signals; consider a usage event in V2 |

## Architecture Decision Records

- [ADR-001: V1 Scope for Config-Driven Keybindings](adrs/adr-001.md) — Hybrid of "foundation +
  conflict relief" plus composer line-editing: context-scoped schema (normal-only wired),
  portable-allowlist validation, single Ctrl-C safety chokepoint, user-scope loading,
  severity-split validation.
- [ADR-002: Parity-First, Two-Wave Delivery](adrs/adr-002.md) — Wave 1 ships line-editing
  defaults + safety chokepoint + data-driven Keys tab (broad value, no config); Wave 2 ships the
  remap engine + config + unbind + `--doctor` validation; presets/leader/modal deferred to V2.

## Open Questions

- **Binding syntax (defer to TechSpec):** the canonical config string format — `"ctrl+l"` vs
  `"C-l"` vs `"Ctrl-L"` — a user-facing contract to settle in the TechSpec.
- **Wave packaging:** the two waves may collapse into a single release if the work is quick —
  acceptable, or keep them as separately shippable increments?
- **Doc copy:** exact `[keybindings]` comment block for `--init-config` and the README section.
- **V2 telemetry:** is adding a keybinding-usage event worth it later to enable real adoption
  metrics, given the project's current no-telemetry stance?
