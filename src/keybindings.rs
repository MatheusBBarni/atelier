//! Shared key vocabulary for config-driven keybindings.
//!
//! This module is the single source of truth for the bindable key surface: a
//! [`KeyChord`] wrapper over crossterm's `KeyEvent`, the `ctrl+k`-style
//! [`parse_key`]/[`format_key`] pair, the portable-key allowlist
//! ([`is_portable`]), the closed [`KeyAction`] vocabulary, the [`DEFAULTS`]
//! binding table, and the [`Keymap`] resolver + [`validate_overrides`].
//!
//! It is intentionally pure and dependency-free with respect to the rest of the
//! crate: it must **not** depend on `tui` or `config` (both depend on *it*). The
//! `KeyAction -> TuiCommand` bridge lives in `tui`; the `[keybindings]` config
//! parsing lives in `config`.

use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A bindable key chord: a crossterm [`KeyEvent`] restricted (by validation) to
/// the portable allowlist.
///
/// The inner `KeyEvent` is always stored in a canonical form (`kind = Press`,
/// `state = empty`) so that two chords describing the same key compare and hash
/// equal regardless of the press/release/repeat metadata a terminal attaches to
/// a live event. Construct chords only through [`KeyChord::new`] /
/// [`KeyChord::from_event`] to preserve that invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord(pub KeyEvent);

impl KeyChord {
    /// Build a canonical chord from a key code and modifiers.
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        // `KeyEvent::new` sets `kind = Press`, `state = empty`.
        KeyChord(KeyEvent::new(code, modifiers))
    }

    /// Normalize a live [`KeyEvent`] into a chord for lookup, discarding the
    /// `kind`/`state` metadata so a binding matches whether the event arrives as
    /// a press, repeat, or release.
    pub fn from_event(event: &KeyEvent) -> Self {
        KeyChord::new(event.code, event.modifiers)
    }
}

/// Closed vocabulary of V1-remappable normal-mode actions.
///
/// Serializes to the kebab-case names used in `[keybindings.normal]`
/// (`toggle-roster`, `scroll-page-up`, …). Adding a remappable action is a
/// deliberate, compiler-guided change: one enum arm here plus one arm in the
/// `KeyAction -> TuiCommand` match in `tui`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyAction {
    ToggleRoster,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    InputLineStart,
    InputLineEnd,
    InputKillToEnd,
    InputKillToStart,
    InputKillWordBack,
}

/// The kebab-case name of an action, matching its serde rename.
///
/// Kept as an exhaustive match (rather than round-tripping through serde) so it
/// is cheap, allocation-free, and usable in `format!`/diagnostics from the
/// config and TUI layers.
pub fn action_name(action: KeyAction) -> &'static str {
    match action {
        KeyAction::ToggleRoster => "toggle-roster",
        KeyAction::ScrollPageUp => "scroll-page-up",
        KeyAction::ScrollPageDown => "scroll-page-down",
        KeyAction::ScrollTop => "scroll-top",
        KeyAction::ScrollBottom => "scroll-bottom",
        KeyAction::InputLineStart => "input-line-start",
        KeyAction::InputLineEnd => "input-line-end",
        KeyAction::InputKillToEnd => "input-kill-to-end",
        KeyAction::InputKillToStart => "input-kill-to-start",
        KeyAction::InputKillWordBack => "input-kill-word-back",
    }
}

/// The V1 default action → key table. With no user overrides, resolving these
/// reproduces today's hardcoded bindings plus the five new editing actions.
pub const DEFAULTS: [(KeyAction, KeyChord); 10] = [
    (
        KeyAction::ToggleRoster,
        KeyChord::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
    ),
    (
        KeyAction::ScrollPageUp,
        KeyChord::new(KeyCode::PageUp, KeyModifiers::NONE),
    ),
    (
        KeyAction::ScrollPageDown,
        KeyChord::new(KeyCode::PageDown, KeyModifiers::NONE),
    ),
    (
        KeyAction::ScrollTop,
        KeyChord::new(KeyCode::Home, KeyModifiers::NONE),
    ),
    (
        KeyAction::ScrollBottom,
        KeyChord::new(KeyCode::End, KeyModifiers::NONE),
    ),
    (
        KeyAction::InputLineStart,
        KeyChord::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
    ),
    (
        KeyAction::InputLineEnd,
        KeyChord::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
    ),
    (
        KeyAction::InputKillToEnd,
        KeyChord::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    ),
    (
        KeyAction::InputKillToStart,
        KeyChord::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    ),
    (
        KeyAction::InputKillWordBack,
        KeyChord::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
    ),
];

/// Per-action user deltas: `Some(chord)` rebinds an action, `None` unbinds it
/// (hands the key back to the terminal). Built by the config layer from
/// `[keybindings.normal]` and consumed by [`Keymap::resolve`].
pub type KeybindingOverrides = BTreeMap<KeyAction, Option<KeyChord>>;

/// Resolved `KeyChord -> KeyAction` lookup, built once at TUI init from the
/// defaults plus validated user overrides.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    lookup: HashMap<KeyChord, KeyAction>,
}

impl Keymap {
    /// Merge `defaults` with `overrides` into a lookup table.
    ///
    /// For each default action: a `Some(chord)` override rebinds it, a `None`
    /// override unbinds it (dropped from the map), and an absent override keeps
    /// the default. The result is consulted by [`Keymap::action_for`].
    pub fn resolve(defaults: &[(KeyAction, KeyChord)], overrides: &KeybindingOverrides) -> Keymap {
        let mut lookup = HashMap::new();
        for (action, chord) in effective_bindings(defaults, overrides) {
            lookup.insert(chord, action);
        }
        Keymap { lookup }
    }

    /// Look up the action bound to a live key event, if any.
    pub fn action_for(&self, key: &KeyEvent) -> Option<KeyAction> {
        self.lookup.get(&KeyChord::from_event(key)).copied()
    }

    /// Iterate the resolved `(action, chord)` bindings, for rendering the Keys
    /// help tab. Order is unspecified; callers that need a stable order sort.
    pub fn entries(&self) -> impl Iterator<Item = (KeyAction, KeyChord)> + '_ {
        self.lookup.iter().map(|(chord, action)| (*action, *chord))
    }
}

/// Resolve `defaults` against `overrides` into the effective `(action, chord)`
/// list, dropping unbound actions. Shared by [`Keymap::resolve`] and
/// [`validate_overrides`] so they cannot disagree about merge semantics.
fn effective_bindings(
    defaults: &[(KeyAction, KeyChord)],
    overrides: &KeybindingOverrides,
) -> Vec<(KeyAction, KeyChord)> {
    let mut out = Vec::with_capacity(defaults.len());
    for (action, default_chord) in defaults {
        match overrides.get(action) {
            Some(Some(chord)) => out.push((*action, *chord)), // rebind
            Some(None) => {}                                  // unbind
            None => out.push((*action, *default_chord)),      // default
        }
    }
    out
}

/// Parse a `ctrl+k`-style key string into a [`KeyChord`].
///
/// Parsing is case-insensitive (`CTRL+K` == `ctrl+k`); modifiers are `+`-joined
/// before the key. Recognized modifiers are `ctrl`, `alt`, and `shift`. The key
/// is a single character (`k`, `1`) or a named key (`pageup`, `home`, `f5`,
/// arrows, …). Whether the result is *bindable* is a separate concern checked by
/// [`is_portable`]/[`validate_overrides`].
pub fn parse_key(s: &str) -> Result<KeyChord> {
    let lower = s.trim().to_ascii_lowercase();
    if lower.is_empty() {
        bail!("empty key binding `{s}` (expected something like `ctrl+k`)");
    }
    let segments: Vec<&str> = lower.split('+').collect();
    // `split` always yields at least one element.
    let (key_seg, mod_segs) = segments.split_last().expect("non-empty split");
    if key_seg.is_empty() {
        bail!("key binding `{s}` is missing a key after `+`");
    }

    let mut modifiers = KeyModifiers::NONE;
    for m in mod_segs {
        match *m {
            "" => bail!("key binding `{s}` has an empty modifier segment"),
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "option" | "meta" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            other => bail!("unknown modifier `{other}` in key binding `{s}`"),
        }
    }

    let code = parse_key_code(key_seg).with_context(|| format!("invalid key binding `{s}`"))?;
    Ok(KeyChord::new(code, modifiers))
}

fn parse_key_code(seg: &str) -> Result<KeyCode> {
    // A single character is a literal key (`k`, `1`, `/`). Checked before the
    // named-key table so the letter `f` is not mistaken for a function key.
    if seg.chars().count() == 1 {
        return Ok(KeyCode::Char(seg.chars().next().expect("len 1")));
    }
    // Function keys: `f1`..`f24` (portability is gated separately).
    if let Some(rest) = seg.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            return Ok(KeyCode::F(n));
        }
    }
    Ok(match seg {
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "tab" => KeyCode::Tab,
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        other => bail!("unknown key `{other}`"),
    })
}

/// Format a chord back to its canonical lowercase `ctrl+k` representation.
/// Round-trips with [`parse_key`] for every allowlisted chord.
pub fn format_key(chord: &KeyChord) -> String {
    let KeyEvent {
        code, modifiers, ..
    } = chord.0;
    let mut out = String::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        out.push_str("ctrl+");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        out.push_str("alt+");
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        out.push_str("shift+");
    }
    let key = match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        other => format!("{other:?}").to_ascii_lowercase(),
    };
    out.push_str(&key);
    out
}

/// Whether a chord is a *portable*, deliverable binding target.
///
/// Allowed: `Ctrl`+letter (except `c`/`d`/`i`/`m`/`[`, which are reserved or
/// terminal-aliased), function keys `F1..F12`, arrow keys, and
/// `PageUp`/`PageDown`/`Home`/`End`. Everything else (`Ctrl`+digit, `Alt`+*,
/// bare characters, …) is rejected at load so users never configure a silent
/// dead key.
pub fn is_portable(chord: &KeyChord) -> bool {
    let KeyEvent {
        code, modifiers, ..
    } = chord.0;
    match code {
        KeyCode::Char(c) => {
            modifiers == KeyModifiers::CONTROL
                && c.is_ascii_alphabetic()
                // C: interrupt, D: EOF, I: == Tab, M: == Enter (`[` is non-alphabetic).
                && !matches!(c.to_ascii_lowercase(), 'c' | 'd' | 'i' | 'm')
        }
        KeyCode::F(n) => (1..=12).contains(&n) && modifiers.is_empty(),
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => modifiers.is_empty(),
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => modifiers.is_empty(),
        _ => false,
    }
}

/// Whether a chord targets a structurally reserved key that can never be
/// rebound. `Ctrl-C` is the interrupt/quit kill-switch and is enforced as a
/// single chokepoint in the TUI; binding it is a hard error.
fn is_reserved(chord: &KeyChord) -> bool {
    chord.0.code == KeyCode::Char('c') && chord.0.modifiers == KeyModifiers::CONTROL
}

/// Hard-validate user overrides against the [`DEFAULTS`] baseline.
///
/// Rejects (with a precise `action`/`key` message):
/// - binding a reserved key (`Ctrl-C`),
/// - binding a non-portable / undeliverable key,
/// - duplicates — two actions resolving to the same key once merged over the
///   defaults.
///
/// Returns `Ok(())` for a clean override set. Called at config load.
pub fn validate_overrides(overrides: &KeybindingOverrides) -> Result<()> {
    // Per-override chord checks. Reserved is checked before portability so a
    // `Ctrl-C` bind reports the security reason rather than "non-portable".
    for (action, binding) in overrides {
        if let Some(chord) = binding {
            if is_reserved(chord) {
                bail!(
                    "keybinding `{}` = `{}` targets a reserved key: Ctrl-C is the interrupt/quit \
                     key and cannot be rebound",
                    action_name(*action),
                    format_key(chord)
                );
            }
            if !is_portable(chord) {
                bail!(
                    "keybinding `{}` = `{}` is not a portable key (allowed: Ctrl+letter except \
                     C/D/I/M, function keys F1-F12, arrows, PageUp/PageDown/Home/End)",
                    action_name(*action),
                    format_key(chord)
                );
            }
        }
    }

    // Duplicate check across the merged defaults + overrides: no two actions may
    // resolve to the same chord.
    let mut seen: HashMap<KeyChord, KeyAction> = HashMap::new();
    for (action, chord) in effective_bindings(&DEFAULTS, overrides) {
        if let Some(prev) = seen.insert(chord, action) {
            bail!(
                "keybinding `{}` is bound to two actions (`{}` and `{}`)",
                format_key(&chord),
                action_name(prev),
                action_name(action)
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> KeyChord {
        parse_key(s).expect("valid chord")
    }

    #[test]
    fn parse_key_handles_modifiers_and_named_keys() {
        assert_eq!(
            parse_key("ctrl+k").unwrap(),
            KeyChord::new(KeyCode::Char('k'), KeyModifiers::CONTROL)
        );
        // Case-insensitive: uppercase parses to the same chord.
        assert_eq!(parse_key("CTRL+K").unwrap(), parse_key("ctrl+k").unwrap());
        assert_eq!(
            parse_key("pageup").unwrap(),
            KeyChord::new(KeyCode::PageUp, KeyModifiers::NONE)
        );
        assert_eq!(
            parse_key("f5").unwrap(),
            KeyChord::new(KeyCode::F(5), KeyModifiers::NONE)
        );
    }

    #[test]
    fn format_key_round_trips_canonical_lowercase() {
        for s in [
            "ctrl+k", "pageup", "home", "f5", "ctrl+a", "end", "pagedown",
        ] {
            assert_eq!(format_key(&chord(s)), s, "round trip for {s}");
        }
    }

    #[test]
    fn parse_key_rejects_malformed_input() {
        assert!(parse_key("ctrl+").is_err());
        assert!(parse_key("squiggle").is_err());
        assert!(parse_key("").is_err());
        assert!(parse_key("hyper+k").is_err());
    }

    #[test]
    fn is_portable_enforces_the_allowlist() {
        assert!(is_portable(&chord("ctrl+a")));
        assert!(is_portable(&chord("f5")));
        assert!(is_portable(&chord("pageup")));
        assert!(is_portable(&chord("home")));
        assert!(is_portable(&chord("up")));

        // Reserved / terminal-aliased Ctrl letters.
        assert!(!is_portable(&chord("ctrl+c")));
        assert!(!is_portable(&chord("ctrl+d")));
        assert!(!is_portable(&chord("ctrl+i")));
        assert!(!is_portable(&chord("ctrl+m")));
        // Non-portable forms.
        assert!(!is_portable(&chord("ctrl+1")));
        assert!(!is_portable(&chord("alt+a")));
        assert!(!is_portable(&chord("f13")));
    }

    #[test]
    fn validate_overrides_flags_reserved_keys() {
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+c")));
        let err = validate_overrides(&o).unwrap_err().to_string();
        assert!(err.contains("reserved"), "got: {err}");
    }

    #[test]
    fn validate_overrides_flags_non_portable_keys() {
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+1")));
        let err = validate_overrides(&o).unwrap_err().to_string();
        assert!(err.contains("portable"), "got: {err}");
    }

    #[test]
    fn validate_overrides_flags_duplicate_bindings() {
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+g")));
        o.insert(KeyAction::ScrollTop, Some(chord("ctrl+g")));
        let err = validate_overrides(&o).unwrap_err().to_string();
        assert!(err.contains("two actions"), "got: {err}");
    }

    #[test]
    fn validate_overrides_detects_collision_with_untouched_default() {
        // Rebinding toggle-roster onto ctrl+a collides with the *untouched*
        // input-line-start default — the merged map must catch it.
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+a")));
        assert!(validate_overrides(&o).is_err());
    }

    #[test]
    fn validate_overrides_accepts_a_valid_map() {
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+g")));
        o.insert(KeyAction::ScrollBottom, None); // unbind is fine
        assert!(validate_overrides(&o).is_ok());
    }

    #[test]
    fn resolve_applies_rebinds_unbinds_and_keeps_defaults() {
        let mut o = KeybindingOverrides::new();
        o.insert(KeyAction::ToggleRoster, Some(chord("ctrl+g"))); // rebind
        o.insert(KeyAction::ScrollTop, None); // unbind
        let map = Keymap::resolve(&DEFAULTS, &o);

        // Rebound: ctrl+g now toggles, the old ctrl+l default is gone.
        assert_eq!(
            map.action_for(&chord("ctrl+g").0),
            Some(KeyAction::ToggleRoster)
        );
        assert_eq!(map.action_for(&chord("ctrl+l").0), None);
        // Unbound: home no longer scrolls to top.
        assert_eq!(map.action_for(&chord("home").0), None);
        // Untouched defaults survive.
        assert_eq!(
            map.action_for(&chord("ctrl+a").0),
            Some(KeyAction::InputLineStart)
        );
        assert_eq!(
            map.action_for(&chord("pageup").0),
            Some(KeyAction::ScrollPageUp)
        );
    }

    #[test]
    fn entries_reflects_the_resolved_map() {
        let map = Keymap::resolve(&DEFAULTS, &KeybindingOverrides::new());
        let entries: Vec<_> = map.entries().collect();
        assert_eq!(entries.len(), DEFAULTS.len());
        assert!(entries
            .iter()
            .any(|(a, c)| *a == KeyAction::InputKillToEnd && format_key(c) == "ctrl+k"));
    }

    #[test]
    fn action_name_matches_serde_rename() {
        // Spot-check the kebab-case contract used by the config schema.
        assert_eq!(action_name(KeyAction::ToggleRoster), "toggle-roster");
        assert_eq!(
            action_name(KeyAction::InputKillWordBack),
            "input-kill-word-back"
        );
    }
}
