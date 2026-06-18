use multiagent::slash_commands::{
    available_commands_summary, catalog, help_command_lines, SlashCommandKind,
};

#[test]
fn catalog_accessor_is_usable_from_an_external_module() {
    let specs = catalog();
    assert_eq!(specs.len(), 13);
    assert_eq!(specs[0].label, "/help");
    assert!(specs
        .iter()
        .any(|spec| spec.label == "/reload:skills" && spec.kind == SlashCommandKind::TuiLocal));
    // /sessions opens the session browser, a TUI-local command (task_09).
    assert!(specs
        .iter()
        .any(|spec| spec.label == "/sessions" && spec.kind == SlashCommandKind::TuiLocal));
}

#[test]
fn command_discovery_includes_provider_status_without_disturbing_others() {
    let specs = catalog();
    // The new command is discoverable as a single app command...
    let provider: Vec<&_> = specs
        .iter()
        .filter(|spec| spec.label == "/provider:status")
        .collect();
    assert_eq!(provider.len(), 1, "provider:status missing from discovery");
    assert_eq!(provider[0].kind, SlashCommandKind::AppCommand);
    assert!(available_commands_summary().contains("/provider:status"));
    assert!(help_command_lines()
        .iter()
        .any(|line| line.contains("/provider:status")));

    // ...and the pre-existing commands are untouched by its insertion.
    for label in [
        "/help",
        "/goal",
        "/goal clear",
        "/config",
        "/subtask",
        "/workflow",
        "/queue",
        "/agent:",
        "/skill:",
        "/reload:skills",
    ] {
        assert!(
            specs.iter().any(|spec| spec.label == label),
            "discovery dropped unrelated command {label}"
        );
    }
}

#[test]
fn catalog_accessor_exposes_stable_static_data_not_mutable_state() {
    // The accessor returns a shared reference to the same static slice every
    // time, so consumers cannot observe or introduce mutation between calls.
    assert!(std::ptr::eq(catalog(), catalog()));
}

#[test]
fn formatting_helpers_cover_the_full_catalog_for_downstream_surfaces() {
    let summary = available_commands_summary();
    let help_lines = help_command_lines();
    for spec in catalog() {
        assert!(
            summary.contains(spec.label),
            "summary missing {}",
            spec.label
        );
        assert!(
            help_lines
                .iter()
                .any(|line| line.starts_with(spec.usage) && line.ends_with(spec.description)),
            "help lines missing {}",
            spec.label
        );
    }
}
