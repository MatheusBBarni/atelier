# TechSpec: Config-Driven Keybindings

## Executive Summary

Introduce a remap layer over the TUI's pure key-routing functions without changing default
behavior. A new shared `src/keybindings.rs` module owns the key vocabulary — a `KeyChord`
(crossterm `KeyEvent` wrapper) with `parse_key`/`format_key`, a closed `KeyAction` enum of the 10
remappable normal-mode actions, a portable-key allowlist, and a `Keymap` (`KeyChord → KeyAction`)
resolved from defaults + user overrides. The config layer parses `[keybindings.normal]`
(`action → key`, `ctrl+k` syntax) and enforces a **user-scope trust boundary** via a new
`ConfigLayer` marker, with severity-split validation. The TUI consults the resolved `Keymap` in
the normal-input branch only, behind a single reserved-key (`Ctrl-C`) guard, and renders the
active map in the Keys help tab.

Delivery is **parity-first (ADR-002)**: Wave 1 ships the five readline editing actions, the
safety chokepoint, and the data-driven Keys tab — all from the *default* keymap, no config. Wave 2
adds the `[keybindings]` config, validation, unbind, and `--doctor` diagnostics. **Primary
trade-off:** the resolved `Keymap` lives on `TuiUiState` (rebuilt per session, no hot-reload),
trading runtime reconfigurability for zero risk to the event-sourced app core and a clean
config-validates / TUI-resolves split.

## System Architecture

### Component Overview

- **`src/keybindings.rs` (new, shared).** `KeyChord`, `parse_key`/`format_key`, portable
  allowlist, `KeyAction` enum, `DEFAULTS` table, `KeybindingOverrides`, `Keymap` + `resolve`,
  `validate_overrides`. No dependency on `tui` or `config` (both depend on *it*).
- **`src/config/mod.rs` (modified).** Parses/validates `[keybindings.normal]` into
  `EffectiveConfig.keybindings` + `keybinding_warnings`; enforces the trust boundary via
  `ConfigLayer`. Depends on `keybindings`.
- **`src/tui/mod.rs` (modified).** Owns the `KeyAction → TuiCommand` exhaustive match, new editing
  command variants + handlers, the reserved-key guard, the normal-context keymap lookup, and the
  data-driven Keys tab. Builds the active `Keymap` onto `TuiUiState` at init.
- **`src/doctor/mod.rs` (modified).** A `config.keybindings` check surfacing warnings.

**Data flow:** `EffectiveConfig.keybindings` (validated overrides) → `Keymap::resolve(DEFAULTS, …)`
at TUI init → stored on `TuiUiState` → consulted by `key_event_to_tui_command_with_ui` (normal
branch) and rendered by `keys_tab_lines`.

## Implementation Design

### Core Interfaces

**Key vocabulary (`src/keybindings.rs`):**
```rust
/// A bindable key chord: a crossterm KeyEvent restricted to the portable allowlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord(pub KeyEvent);

pub fn parse_key(s: &str) -> Result<KeyChord, KeyParseError>; // "ctrl+k" → chord (case-insensitive)
pub fn format_key(chord: &KeyChord) -> String;               // chord → "ctrl+k" (canonical)
pub fn is_portable(chord: &KeyChord) -> bool;                // Ctrl+letter (≠C/D/I/M/[), F-keys, arrows, Pg/Home/End

/// Closed vocabulary of V1-remappable normal-mode actions; serde renames to kebab-case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyAction {
    ToggleRoster, ScrollPageUp, ScrollPageDown, ScrollTop, ScrollBottom,
    InputLineStart, InputLineEnd, InputKillToEnd, InputKillToStart, InputKillWordBack,
}
```

**Resolution (`src/keybindings.rs`):**
```rust
/// Per-action user deltas: Some(chord) = rebind, None = unbind. Built by the config layer.
pub type KeybindingOverrides = BTreeMap<KeyAction, Option<KeyChord>>;

pub struct Keymap { lookup: HashMap<KeyChord, KeyAction> }

impl Keymap {
    pub fn resolve(defaults: &[(KeyAction, KeyChord)], overrides: &KeybindingOverrides) -> Keymap;
    pub fn action_for(&self, key: &KeyEvent) -> Option<KeyAction>;
    pub fn entries(&self) -> impl Iterator<Item = (KeyAction, KeyChord)>; // for the Keys tab
}

/// Hard-validates merged defaults+overrides (reserved / non-portable / duplicate). Used at load.
pub fn validate_overrides(overrides: &KeybindingOverrides) -> Result<(), KeybindingError>;
```

**New TUI commands (`src/tui/mod.rs`):** extend the existing enums and add the action bridge:
```rust
enum InputCursorCommand { Left, Right, Up, Down, LineStart, LineEnd } // + LineStart/LineEnd
enum InputKillCommand { ToLineEnd, ToLineStart, WordBack }
// TuiCommand gains: InputKill(InputKillCommand)

/// The only place KeyAction meets TuiCommand — exhaustive, so names can't drift.
fn command_for_action(action: KeyAction) -> TuiCommand;
```

**Config trust marker (`src/config/mod.rs`):**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigLayer { Builtin, Cli, Home, Local }
// apply_raw(&mut self, raw: RawConfig, source_dir: &Path, source_name: &str, layer: ConfigLayer)
//   → applies [keybindings] only when layer != Local; else records a keybinding_warning.
```

### Data Models

**Config schema (user-facing contract):**
```toml
[keybindings.normal]          # only the `normal` context is honored in V1
toggle-roster     = "ctrl+g"  # remap an action's key
input-kill-to-end = "ctrl+k"
help-open         = false      # unbind (hand the key back to the terminal)
```

**`RawConfig` addition (`src/config/mod.rs:417`, keeps `deny_unknown_fields`):**
```rust
keybindings: Option<BTreeMap<String, BTreeMap<String, RawKeyBinding>>>, // context → action → value
// RawKeyBinding = Key(String) | Disabled(bool)  (untagged: accepts "ctrl+g" or false)
```

**`EffectiveConfig` additions (`src/config/mod.rs:402`):**
```rust
pub keybindings: KeybindingOverrides,        // validated, post-merge deltas (Home/Cli only)
pub keybinding_warnings: Vec<String>,        // soft-fails + "ignored local [keybindings]" notes
```

**Default action→key table (`DEFAULTS`, in `keybindings.rs`)** — the V1 baseline:
`toggle-roster=ctrl+l`, `scroll-page-up=pageup`, `scroll-page-down=pagedown`, `scroll-top=home`,
`scroll-bottom=end`, `input-line-start=ctrl+a`, `input-line-end=ctrl+e`,
`input-kill-to-end=ctrl+k`, `input-kill-to-start=ctrl+u`, `input-kill-word-back=ctrl+w`.

### API Endpoints

N/A — no network API. The user-facing surfaces are the TOML schema above, `atelier --print-config`
(emits the effective keymap), `atelier --init-config` (commented `[keybindings]` block), and
`atelier --doctor` (`config.keybindings` check).

## Integration Points

N/A — no external services. All integration is internal (config ↔ keybindings ↔ tui/doctor).

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|-----------|-------------|----------------------|-----------------|
| `src/keybindings.rs` | new | Shared vocabulary/Keymap/validation. Low risk (pure, unit-tested). | Create module |
| `src/tui/mod.rs` routing (`key_event_to_tui_command_with_ui:1056`) | modified | Add reserved-key guard + normal-context keymap lookup. **Med risk (hot file)**; defaults must stay identical. | Consolidate 3× `Ctrl-C` (`1070/1123/1394`) into one guard; add lookup-before-fallback |
| `src/tui/mod.rs` enums + exec match (`169`/`203`/`~669-723`) | modified | New `InputCursorCommand`/`InputKill` variants + handlers (`kill_input`). Low risk. | Add variants, handlers, `command_for_action` |
| `src/tui/mod.rs` Keys tab (`keys_tab_lines:3737` + test `11685`) | modified | Render from active `Keymap` via `format_key`. Low risk. | Data-drive; rewrite test to "no config ⇒ default keys present" |
| `src/tui/mod.rs` `TuiUiState` (`291`) | modified | Holds resolved `Keymap`, built at init from `EffectiveConfig`. Low risk. | Add field + construction |
| `src/config/mod.rs` (`apply_raw:909`, `load_effective_config:1647`, `apply_config_file:1713`) | modified | `ConfigLayer` param; parse/validate keybindings; user-scope gate. **Med risk** (signature churn across call sites). | Thread `ConfigLayer`; add parse+validate; warnings |
| `src/config/mod.rs` emit (`starter_config_text:2195`, `PrintableConfig:1956`, `build_printable_config:2035`) | modified | Document + emit effective keymap. Low risk. | Add `[keybindings]` block + field |
| `src/doctor/mod.rs` (`run_doctor:55`) | modified | `config.keybindings` warning check. Low risk. | Add check from `keybinding_warnings` |

## Testing Approach

### Unit Tests
- **`keybindings.rs`:** `parse_key`/`format_key` round-trip across the allowlist; `is_portable`
  rejects `ctrl+c/d/i/m/[`, `ctrl+digit`, `alt+*`; `validate_overrides` flags reserved-key binds,
  non-portable keys, and duplicates (two actions → one key); `Keymap::resolve` applies rebinds,
  unbinds, and leaves untouched defaults intact.
- **Editing handlers (`tui`):** `kill_input` for `ToLineEnd`/`ToLineStart`/`WordBack` and the new
  cursor `LineStart`/`LineEnd` against multi-byte UTF-8 input and edge cursors (0, len).
- **Default fidelity (regression):** with empty overrides, the resolved `Keymap` + routing produce
  the exact pre-feature `(KeyEvent → TuiCommand)` set; reserved-key guard returns interrupt for
  `Ctrl-C` before any lookup.
- **Keys tab:** renders one line per active binding via `format_key`; reserved keys shown locked;
  no `Color::` literals (honors `colors_live_only_in_theme_module`).

### Integration Tests
- **Config (`tests/cli.rs`-style):** a home-config `[keybindings.normal]` round-trips into routed
  behavior; an invalid binding hard-fails `load_effective_config` with a `file+field+value`
  message; an **unknown action** soft-fails (kept running, warning present); a **local
  `./atelier.toml` `[keybindings]` is ignored** with a warning; `--print-config` shows the merged
  effective keymap; `--doctor` emits a `config.keybindings` warning when warnings exist.

## Development Sequencing

### Build Order
1. **`src/keybindings.rs` foundation** — `KeyChord`, `parse_key`/`format_key`, allowlist,
   `KeyAction`, `DEFAULTS`, `Keymap`/`resolve`, `KeybindingOverrides`, `validate_overrides`. No
   dependencies. *(Wave 1 uses defaults/format/resolve; Wave 2 uses parse/validate.)*
2. **Editing commands + handlers** — add `InputCursorCommand::LineStart/LineEnd`,
   `TuiCommand::InputKill(InputKillCommand)`, and `kill_input`; wire execution arms. Depends on: none.
3. **Reserved-key chokepoint** — consolidate the three `Ctrl-C` branches into one pre-lookup guard.
   Depends on: none.
4. **Default keymap wiring** — `command_for_action` match; store a `Keymap` (from `DEFAULTS`) on
   `TuiUiState`; consult it in the normal-input branch behind the step-3 guard; bind the new
   editing actions by default. Depends on: 1, 2, 3.
5. **Data-driven Keys tab** — `keys_tab_lines(keymap, theme)` via `format_key`; rewrite its test.
   Depends on: 1, 4. **→ Wave 1 complete (broad parity, no config).**
6. **Config section + trust boundary** — add `keybindings` to `RawConfig`; introduce `ConfigLayer`
   threaded through `apply_config_file`/`apply_raw`; apply only when `layer != Local`, else warn.
   Depends on: 1.
7. **Validation + EffectiveConfig** — parse overrides; `validate_overrides` hard-fails at load;
   collect soft + local-ignored warnings; store `keybindings` + `keybinding_warnings` on
   `EffectiveConfig`. Depends on: 6, 1.
8. **Resolve customizations** — build the active `Keymap` from `EffectiveConfig.keybindings`;
   routing + Keys tab now reflect overrides/unbinds. Depends on: 4, 5, 7.
9. **Doctor + emit surfaces** — `config.keybindings` check + startup notice; `PrintableConfig`
   field + `build_printable_config`; `[keybindings]` block in `starter_config_text`. Depends on: 7.
   **→ Wave 2 complete.**

### Technical Dependencies
None external. Step 1 is the only blocking shared dependency; everything else builds on it. No new
crates (crossterm, serde, toml already present).

## Monitoring and Observability

No usage telemetry exists or is added (per PRD). Operational visibility = the `config.keybindings`
`DoctorCheck` (status `Warn`, `context` carrying the offending entries), the one-line TUI startup
notice when warnings exist, and the test suite as the correctness gate. Hard validation failures
surface as the precise config-load error on both startup and `--doctor`.

## Technical Considerations

### Key Decisions
- **Decision:** Shared `keybindings` module with a closed `KeyAction` enum; `KeyAction → TuiCommand`
  via an exhaustive match. **Rationale:** compiler-enforced no-drift; keeps `config` free of any
  `tui` dependency. **Trade-off:** adding a remappable action later needs an enum + match arm.
  **Rejected:** string-typed actions (runtime drift).
- **Decision:** Resolved `Keymap` on `TuiUiState`, consulted in the normal branch only.
  **Rationale:** zero risk to the app/event core; context-gating keeps the keymap out of modal
  contexts (security). **Trade-off:** no hot-reload. **Rejected:** keymap on `AppState`.
- **Decision:** `ConfigLayer`-gated user-scope + severity-split validation. **Rationale:** untrusted
  project config can't touch control keys; cosmetic typos don't brick the TUI. **Trade-off:**
  signature churn across config call sites. **Rejected:** inline strip (no provenance); hard-error
  on local (DoS).
- **Decision:** Portable allowlist, reject the rest at load. **Rationale:** no silent dead keys.
  **Trade-off:** `Alt`/`Ctrl+digit`/Kitty chords deferred to V2.

### Known Risks
- **Hot-file churn in `src/tui/mod.rs`** (likely) → keep the lookup a small addition behind the
  reserved guard; the default-fidelity regression test pins behavior; sequence after other
  in-flight TUI branches.
- **`Ctrl-U` semantics** (kill-to-start) may surprise → documented in `--init-config` + Keys tab.
- **Threading `ConfigLayer` misses a call site** → integration test asserts local `[keybindings]`
  is ignored + warned.
- **Action↔command drift** → the exhaustive match makes it a compile error.

## Architecture Decision Records

- [ADR-001: V1 Scope for Config-Driven Keybindings](adrs/adr-001.md) — Hybrid foundation +
  composer line-editing; context-scoped schema, allowlist, single Ctrl-C chokepoint, user-scope,
  severity-split.
- [ADR-002: Parity-First, Two-Wave Delivery](adrs/adr-002.md) — Wave 1 parity (defaults +
  chokepoint + data-driven Keys tab), Wave 2 remap engine + config + unbind + doctor.
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — `action → key` schema, closed
  `KeyAction` enum + exhaustive `TuiCommand` match, `keys` module, `TuiUiState`-resident `Keymap`
  consulted before the cascade.
- [ADR-004: Config Trust Boundary and Validation Severity](adrs/adr-004.md) — `ConfigLayer` gates
  keybindings to non-local layers (ignore + warn on local); hard-fail security/structural,
  soft-fail cosmetic; structural reserved-key guard.
