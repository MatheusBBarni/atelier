//! Branded welcome screen, rendered as a synthetic `ChatItemKind::Welcome`
//! chat item (ADR-005). This module isolates the `tui-big-text` dependency
//! (ADR-004): the wordmark is drawn into an off-screen [`Buffer`] and converted
//! to `Line`s so it scrolls with chat history like any other item, instead of
//! painting a fixed region.
//!
//! Colors come exclusively from theme tokens (no inline color literals — the
//! task_03 source invariant enforces this).

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use tui_big_text::{BigText, PixelSize};

use super::theme::Theme;
use crate::app::AgentView;

const WORDMARK: &str = "Atelier";
/// Full-size wordmark at or above this width; compact between this and
/// [`COMPACT_MIN_WIDTH`]; plain styled text below (PRD F1 ladder).
const FULL_MIN_WIDTH: u16 = 80;
const COMPACT_MIN_WIDTH: u16 = 60;
/// Glyph heights in terminal cells for the two `tui-big-text` sizes used.
const FULL_HEIGHT: u16 = 8;
const COMPACT_HEIGHT: u16 = 4;
/// Maximum agent names listed inline before collapsing to "+N more".
const AGENT_PREVIEW: usize = 4;

/// Facts shown beneath the wordmark, borrowed from live app state at render
/// time. `git` is the cross-task integration point: `None` until task_05 wires
/// `AppState.git_context`, at which point the caller passes the repo/branch.
pub struct WelcomeFacts<'a> {
    pub version: &'a str,
    pub working_directory: Option<&'a Path>,
    pub agents: &'a [AgentView],
    pub preset: Option<&'a str>,
    pub warnings: usize,
    pub git: Option<(&'a str, &'a str)>,
}

/// Render the welcome item: an adaptive wordmark (skipped under `NO_COLOR` or
/// `hide_banner`) followed by the facts box. `width` is the chat content width
/// the lines render into, so the wordmark always fits.
pub fn welcome_lines(
    theme: &Theme,
    width: u16,
    hide_banner: bool,
    facts: &WelcomeFacts,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if !hide_banner && !theme.is_monochrome() {
        lines.extend(wordmark_lines(theme, width));
        lines.push(Line::from(""));
    }
    lines.extend(facts_lines(theme, facts));
    lines
}

/// Select the wordmark form by available width: full lettering, a compact
/// half-size, or a single plain styled line on narrow terminals.
fn wordmark_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    if width >= FULL_MIN_WIDTH {
        big_text_lines(theme, width, PixelSize::Full, FULL_HEIGHT)
    } else if width >= COMPACT_MIN_WIDTH {
        big_text_lines(theme, width, PixelSize::Quadrant, COMPACT_HEIGHT)
    } else {
        vec![plain_wordmark(theme)]
    }
}

/// Render `tui-big-text` into an off-screen buffer of the wordmark's size, then
/// lift each buffer row into a styled `Line` so it lives inside the chat
/// `Paragraph` and scrolls with history.
fn big_text_lines(
    theme: &Theme,
    width: u16,
    pixel_size: PixelSize,
    height: u16,
) -> Vec<Line<'static>> {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    BigText::builder()
        .pixel_size(pixel_size)
        .alignment(Alignment::Left)
        .style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .lines(vec![Line::from(WORDMARK)])
        .build()
        .render(area, &mut buffer);
    trim_blank_edges(buffer_to_lines(&buffer))
}

/// Drop fully-blank leading/trailing rows from a rendered wordmark. The glyph
/// buffer is a fixed height (so the font has room to paint), but `font8x8`
/// leaves the bottom row empty for ascender-only words like "Atelier"; that dead
/// row plus the welcome separator read as excess space before the facts box.
/// Interior rows are kept (they carry glyph structure).
fn trim_blank_edges(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let is_blank = |line: &Line<'static>| {
        line.spans
            .iter()
            .all(|span| span.content.chars().all(char::is_whitespace))
    };
    match (
        lines.iter().position(|line| !is_blank(line)),
        lines.iter().rposition(|line| !is_blank(line)),
    ) {
        (Some(first), Some(last)) => lines[first..=last].to_vec(),
        // Defensive: an all-blank render keeps its rows rather than vanishing.
        _ => lines,
    }
}

/// Convert a rendered buffer into owned `Line`s, coalescing runs of same-styled
/// cells into single spans.
fn buffer_to_lines(buffer: &Buffer) -> Vec<Line<'static>> {
    let area = *buffer.area();
    (0..area.height)
        .map(|y| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut text = String::new();
            let mut run_style: Option<Style> = None;
            for x in 0..area.width {
                let cell = &buffer[(x, y)];
                let style = cell.style();
                if run_style != Some(style) {
                    if let Some(prev) = run_style.take() {
                        spans.push(Span::styled(std::mem::take(&mut text), prev));
                    }
                    run_style = Some(style);
                }
                text.push_str(cell.symbol());
            }
            if let Some(prev) = run_style {
                spans.push(Span::styled(text, prev));
            }
            Line::from(spans)
        })
        .collect()
}

fn plain_wordmark(theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        WORDMARK.to_string(),
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Build the facts box. Pure over its inputs so it is unit-testable for the
/// git Some/None and agent-count behaviors without a render.
fn facts_lines(theme: &Theme, facts: &WelcomeFacts) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            "atelier ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{}", facts.version),
            Style::default().fg(theme.text_muted),
        ),
    ]));
    if let Some(dir) = facts.working_directory {
        lines.push(fact_line(theme, "cwd", &dir.display().to_string()));
    }
    if let Some((repo, branch)) = facts.git {
        lines.push(fact_line(theme, "repo", &format!("{repo} · {branch}")));
    }
    lines.push(fact_line(theme, "agents", &agents_summary(facts.agents)));
    if let Some(preset) = facts.preset {
        lines.push(fact_line(theme, "preset", preset));
    }
    if facts.warnings > 0 {
        lines.push(Line::from(vec![
            Span::styled("warnings: ", Style::default().fg(theme.text_dim)),
            Span::styled(
                facts.warnings.to_string(),
                Style::default().fg(theme.status_warn),
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "type /help for commands",
        Style::default().fg(theme.text_muted),
    )));
    lines
}

fn fact_line(theme: &Theme, label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(theme.text_dim)),
        Span::styled(value.to_string(), Style::default().fg(theme.text_muted)),
    ])
}

fn agents_summary(agents: &[AgentView]) -> String {
    if agents.is_empty() {
        return "0".to_string();
    }
    let names: Vec<&str> = agents
        .iter()
        .take(AGENT_PREVIEW)
        .map(|agent| agent.name.as_str())
        .collect();
    let extra = agents.len().saturating_sub(names.len());
    let mut summary = format!("{} · {}", agents.len(), names.join(", "));
    if extra > 0 {
        summary.push_str(&format!(", +{extra} more"));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::TerminalCaps;

    fn truecolor() -> Theme {
        Theme::resolve(TerminalCaps {
            no_color: false,
            truecolor: true,
        })
    }

    fn agent(name: &str) -> AgentView {
        AgentView {
            id: name.to_string(),
            name: name.to_string(),
            runtime: "fake".to_string(),
            model: "default".to_string(),
            effort: "medium".to_string(),
            thinking: false,
            capabilities: Vec::new(),
            availability: None,
            status: "idle".to_string(),
        }
    }

    fn facts_with<'a>(
        agents: &'a [AgentView],
        git: Option<(&'a str, &'a str)>,
    ) -> WelcomeFacts<'a> {
        WelcomeFacts {
            version: env!("CARGO_PKG_VERSION"),
            working_directory: None,
            agents,
            preset: Some("default"),
            warnings: 0,
            git,
        }
    }

    fn line_text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn width_ladder_selects_full_compact_or_plain() {
        let theme = truecolor();
        let full = wordmark_lines(&theme, 100);
        let compact = wordmark_lines(&theme, 70);
        // Full keeps the most rows but is trimmed of its dead bottom row, so it
        // is shorter than the raw buffer height and taller than compact.
        assert!(full.len() < FULL_HEIGHT as usize);
        assert!(full.len() > compact.len());
        assert_eq!(compact.len(), COMPACT_HEIGHT as usize);

        let plain = wordmark_lines(&theme, 50);
        assert_eq!(plain.len(), 1);
        assert!(line_text(&plain[0]).contains("Atelier"));
    }

    #[test]
    fn wordmark_has_no_blank_edge_rows() {
        let theme = truecolor();
        let blank = |line: &Line| {
            line.spans
                .iter()
                .all(|span| span.content.chars().all(char::is_whitespace))
        };
        for width in [100, 70] {
            let lines = wordmark_lines(&theme, width);
            assert!(
                !blank(lines.first().unwrap()),
                "leading row blank at {width}"
            );
            assert!(
                !blank(lines.last().unwrap()),
                "trailing row blank at {width}"
            );
        }
    }

    #[test]
    fn facts_box_includes_version_agent_count_and_preset() {
        let agents = [agent("explorer"), agent("fixer"), agent("oracle")];
        let facts = facts_with(&agents, None);
        let text: String = facts_lines(&truecolor(), &facts)
            .iter()
            .map(|line| format!("{}\n", line_text(line)))
            .collect();

        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains('3')); // agent count
        assert!(text.contains("explorer"));
        assert!(text.contains("default")); // preset
    }

    #[test]
    fn facts_box_omits_repo_line_without_git() {
        let agents = [agent("explorer")];
        let none: String = facts_lines(&truecolor(), &facts_with(&agents, None))
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!none.contains("repo:"));

        let some: String = facts_lines(
            &truecolor(),
            &facts_with(&agents, Some(("atelier", "main"))),
        )
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n");
        assert!(some.contains("repo:"));
        assert!(some.contains("atelier · main"));
    }

    #[test]
    fn hide_banner_suppresses_wordmark_but_keeps_facts() {
        let agents = [agent("explorer")];
        let facts = facts_with(&agents, None);
        let lines = welcome_lines(&truecolor(), 100, true, &facts);
        let text: String = lines
            .iter()
            .map(|line| format!("{}\n", line_text(line)))
            .collect();

        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        // No big-text rows: every line is facts (≤ a handful), far fewer than a
        // full wordmark's 8 rows plus facts.
        assert!(lines.len() < FULL_HEIGHT as usize);
    }

    #[test]
    fn no_color_theme_skips_wordmark_with_identical_facts_text() {
        let agents = [agent("explorer")];
        let facts = facts_with(&agents, None);
        let mono = Theme::resolve(TerminalCaps {
            no_color: true,
            truecolor: false,
        });
        let color_facts: String = welcome_lines(&truecolor(), 100, true, &facts)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");
        let mono_lines = welcome_lines(&mono, 100, false, &facts);
        let mono_text: String = mono_lines
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            mono_lines.len() < FULL_HEIGHT as usize,
            "no wordmark under no-color"
        );
        assert_eq!(color_facts, mono_text);
    }
}
