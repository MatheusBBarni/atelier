use multiagent::slash_commands::{
    available_commands_summary, catalog, help_command_lines, SlashCommandKind,
};

#[test]
fn catalog_accessor_is_usable_from_an_external_module() {
    let specs = catalog();
    assert_eq!(specs.len(), 8);
    assert_eq!(specs[0].label, "/help");
    assert!(specs
        .iter()
        .any(|spec| spec.label == "/reload:skills" && spec.kind == SlashCommandKind::TuiLocal));
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
