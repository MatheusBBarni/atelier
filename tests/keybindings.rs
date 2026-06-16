//! Integration coverage for the keybindings foundation: exercising the public
//! API (`parse_key` → `Keymap::resolve` → `action_for`) end-to-end, the way the
//! config and TUI layers will consume it.

use multiagent::keybindings::{
    format_key, parse_key, validate_overrides, KeyAction, KeybindingOverrides, Keymap, DEFAULTS,
};

#[test]
fn parse_then_resolve_round_trips_into_the_lookup() {
    // A user remaps toggle-roster onto ctrl+g and unbinds scroll-top (home).
    let mut overrides = KeybindingOverrides::new();
    overrides.insert(
        KeyAction::ToggleRoster,
        Some(parse_key("ctrl+g").expect("ctrl+g parses")),
    );
    overrides.insert(KeyAction::ScrollTop, None);

    // The override set is valid, so resolution mirrors the config-load path.
    validate_overrides(&overrides).expect("override set is valid");
    let map = Keymap::resolve(&DEFAULTS, &overrides);

    // ctrl+g now toggles the roster…
    let ctrl_g = parse_key("ctrl+g").unwrap();
    assert_eq!(map.action_for(&ctrl_g.0), Some(KeyAction::ToggleRoster));
    // …and the displaced ctrl+l default is no longer bound.
    let ctrl_l = parse_key("ctrl+l").unwrap();
    assert_eq!(map.action_for(&ctrl_l.0), None);
    // The unbound scroll-top (home) is gone entirely.
    let home = parse_key("home").unwrap();
    assert_eq!(map.action_for(&home.0), None);

    // Untouched defaults still resolve.
    let ctrl_a = parse_key("ctrl+a").unwrap();
    assert_eq!(map.action_for(&ctrl_a.0), Some(KeyAction::InputLineStart));
}

#[test]
fn default_keymap_reproduces_every_default_binding() {
    let map = Keymap::resolve(&DEFAULTS, &KeybindingOverrides::new());
    for (action, chord) in DEFAULTS {
        assert_eq!(
            map.action_for(&chord.0),
            Some(action),
            "default `{}` should resolve",
            format_key(&chord)
        );
    }
}
