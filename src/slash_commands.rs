//! Shared metadata-only catalog of the built-in slash commands.
//!
//! This module is the single source of visible command metadata for the TUI
//! command dropdown, the TUI help overlay, and app unknown-command guidance.
//! Per ADR-003 it must stay metadata-only: no dispatch callbacks, runtime
//! state, or command execution. Execution ownership is unchanged — the TUI
//! handles its local commands, `App::submit_prompt` handles app commands, and
//! `/agent:` plus `/skill:` remain prompt prefixes.

/// Where a slash command is handled. Categorization only; the catalog never
/// dispatches, so adding a kind here must not move command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommandKind {
    /// Handled inside the TUI without sending an app event.
    TuiLocal,
    /// Handled by the app when the prompt is submitted.
    AppCommand,
    /// A prompt prefix completed by a specialized dropdown, not a command.
    PromptPrefix,
}

/// Static metadata for one built-in slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    /// Canonical command name shown in dropdown rows.
    pub label: &'static str,
    /// Text inserted into the composer when the command is accepted.
    pub insert_text: &'static str,
    /// Invocation shape including argument placeholders.
    pub usage: &'static str,
    /// Short human-readable purpose, shared across help and guidance.
    pub description: &'static str,
    /// Which surface owns execution of this command.
    pub kind: SlashCommandKind,
}

/// The fixed V1 catalog.
///
/// ADR-001 froze the original set on 2026-06-06. Amended 2026-06-11 (approved
/// scope change) to add `/workflow` and `/queue`, which shipped as real app
/// commands after the freeze and were already visible in app guidance and the
/// help overlay; keeping them out of the catalog would have dropped real
/// commands from the surfaces this catalog is meant to align. Amended
/// 2026-06-13 (Provider Usage Status PRD scope change) to add
/// `/provider:status`, a runway-first provider readiness command routed as an
/// app command. Amended 2026-06-17 (Session Browser & Resume PRD scope change)
/// to add `/sessions`, a TUI-local command opening the session browser. Entries
/// must still not be added or removed without a deliberate PRD scope change.
const CATALOG: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        label: "/help",
        insert_text: "/help",
        usage: "/help",
        description: "toggle the help overlay",
        kind: SlashCommandKind::TuiLocal,
    },
    SlashCommandSpec {
        label: "/goal",
        insert_text: "/goal",
        usage: "/goal | /goal <text>",
        description: "show or set the session goal",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/goal clear",
        insert_text: "/goal clear",
        usage: "/goal clear",
        description: "clear the session goal",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/config",
        insert_text: "/config",
        usage: "/config",
        description: "show config files, preset, warnings",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/subtask",
        insert_text: "/subtask",
        usage: "/subtask <agent> <task>",
        description: "run a bounded child task",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/workflow",
        insert_text: "/workflow",
        usage: "/workflow <prompt>",
        description: "execute a broad prompt with workflow evidence",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/queue",
        insert_text: "/queue",
        usage: "/queue <message>",
        description: "queue a follow-up prompt (alias /q)",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/sessions",
        insert_text: "/sessions",
        usage: "/sessions",
        description: "browse and preview past sessions",
        kind: SlashCommandKind::TuiLocal,
    },
    SlashCommandSpec {
        label: "/provider:status",
        insert_text: "/provider:status",
        usage: "/provider:status",
        description: "show provider readiness and usage runway",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/trust",
        insert_text: "/trust",
        usage: "/trust | /trust revoke <n> | /trust clear",
        description: "list, revoke, or clear session-trusted actions",
        kind: SlashCommandKind::AppCommand,
    },
    SlashCommandSpec {
        label: "/agent:",
        insert_text: "/agent:",
        usage: "/agent:<agent_name>",
        description: "select an enabled agent for this prompt",
        kind: SlashCommandKind::PromptPrefix,
    },
    SlashCommandSpec {
        label: "/skill:",
        insert_text: "/skill:",
        usage: "/skill:<skill_name>",
        description: "load skill context",
        kind: SlashCommandKind::PromptPrefix,
    },
    SlashCommandSpec {
        label: "/reload:skills",
        insert_text: "/reload:skills",
        usage: "/reload:skills",
        description: "refresh cached skill names",
        kind: SlashCommandKind::TuiLocal,
    },
];

/// The fixed V1 slash command catalog in display order.
pub fn catalog() -> &'static [SlashCommandSpec] {
    CATALOG
}

/// Comma-joined usage list for unknown-command guidance.
pub fn available_commands_summary() -> String {
    let usages: Vec<&str> = CATALOG.iter().map(|spec| spec.usage).collect();
    usages.join(", ")
}

/// One line per command pairing usage with its description, with the usage
/// column padded so descriptions align in the help overlay.
pub fn help_command_lines() -> Vec<String> {
    let usage_width = CATALOG
        .iter()
        .map(|spec| spec.usage.chars().count())
        .max()
        .unwrap_or(0);
    CATALOG
        .iter()
        .map(|spec| format!("{:usage_width$}  {}", spec.usage, spec.description))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXED_V1_LABELS: [&str; 13] = [
        "/help",
        "/goal",
        "/goal clear",
        "/config",
        "/subtask",
        "/workflow",
        "/queue",
        "/sessions",
        "/provider:status",
        "/trust",
        "/agent:",
        "/skill:",
        "/reload:skills",
    ];

    #[test]
    fn catalog_labels_are_exactly_the_fixed_v1_set() {
        let labels: Vec<&str> = catalog().iter().map(|spec| spec.label).collect();
        assert_eq!(labels, FIXED_V1_LABELS);
    }

    #[test]
    fn every_entry_has_complete_metadata() {
        for spec in catalog() {
            assert!(!spec.label.is_empty(), "empty label");
            assert!(
                spec.label.starts_with('/'),
                "label {} must start with /",
                spec.label
            );
            assert!(
                !spec.insert_text.is_empty(),
                "{}: empty insert text",
                spec.label
            );
            assert!(
                spec.insert_text.starts_with('/'),
                "{}: insert text must start with /",
                spec.label
            );
            assert!(!spec.usage.is_empty(), "{}: empty usage", spec.label);
            assert!(
                spec.usage.starts_with(spec.label),
                "{}: usage must begin with the label",
                spec.label
            );
            assert!(
                !spec.description.is_empty(),
                "{}: empty description",
                spec.label
            );
        }
    }

    #[test]
    fn prompt_prefix_entries_are_categorized_as_prompt_prefixes() {
        let prefixes: Vec<&SlashCommandSpec> = catalog()
            .iter()
            .filter(|spec| spec.kind == SlashCommandKind::PromptPrefix)
            .collect();
        let labels: Vec<&str> = prefixes.iter().map(|spec| spec.label).collect();
        assert_eq!(labels, ["/agent:", "/skill:"]);
        for spec in prefixes {
            assert!(
                spec.label.ends_with(':'),
                "{}: prompt prefixes end with a colon so the specialized \
                 dropdowns take over after insertion",
                spec.label
            );
            assert_eq!(
                spec.insert_text, spec.label,
                "{}: prefix insertion must hand off without extra text",
                spec.label
            );
        }
    }

    #[test]
    fn tui_local_and_app_command_entries_are_categorized_distinctly() {
        let labels_of = |kind: SlashCommandKind| -> Vec<&str> {
            catalog()
                .iter()
                .filter(|spec| spec.kind == kind)
                .map(|spec| spec.label)
                .collect()
        };
        assert_eq!(
            labels_of(SlashCommandKind::TuiLocal),
            ["/help", "/sessions", "/reload:skills"]
        );
        assert_eq!(
            labels_of(SlashCommandKind::AppCommand),
            [
                "/goal",
                "/goal clear",
                "/config",
                "/subtask",
                "/workflow",
                "/queue",
                "/provider:status",
                "/trust"
            ]
        );
    }

    #[test]
    fn available_commands_summary_includes_every_label() {
        let summary = available_commands_summary();
        for label in FIXED_V1_LABELS {
            assert!(
                summary.contains(label),
                "summary missing {label}: {summary}"
            );
        }
    }

    #[test]
    fn help_command_lines_pair_usage_with_aligned_descriptions() {
        let lines = help_command_lines();
        assert_eq!(lines.len(), catalog().len());
        let usage_width = catalog()
            .iter()
            .map(|spec| spec.usage.chars().count())
            .max()
            .unwrap();
        for (line, spec) in lines.iter().zip(catalog()) {
            assert!(line.starts_with(spec.usage), "line missing usage: {line}");
            let description: String = line.chars().skip(usage_width + 2).collect();
            assert_eq!(description, spec.description, "misaligned line: {line}");
        }
    }

    #[test]
    fn provider_status_is_a_single_app_command_entry() {
        let provider_entries: Vec<&SlashCommandSpec> = catalog()
            .iter()
            .filter(|spec| spec.label == "/provider:status")
            .collect();
        assert_eq!(
            provider_entries.len(),
            1,
            "expected exactly one /provider:status catalog entry"
        );
        let spec = provider_entries[0];
        assert!(!spec.description.is_empty(), "empty description");
        assert!(!spec.usage.is_empty(), "empty usage");
        assert_eq!(spec.insert_text, "/provider:status");
        // The colon is part of a namespaced command name, not a prompt-prefix
        // handoff: the command stays an app command and must not end with ':'
        // (which is what the `/agent:`/`/skill:` dropdowns key off of).
        assert_eq!(spec.kind, SlashCommandKind::AppCommand);
        assert!(
            !spec.label.ends_with(':'),
            "/provider:status must not look like a prompt prefix"
        );
    }

    #[test]
    fn catalog_lookup_for_unknown_commands_finds_nothing() {
        // The catalog is a closed set, so unrelated commands have no entry and
        // unknown-command guidance still treats them as unknown. Adding
        // `/provider:status` must not register partial or sibling labels.
        for unknown in ["/provider", "/status", "/doctor", "/provider:usage"] {
            assert!(
                catalog().iter().all(|spec| spec.label != unknown),
                "{unknown} must not be a catalog entry"
            );
        }
    }
}
