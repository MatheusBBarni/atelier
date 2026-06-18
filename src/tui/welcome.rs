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

/// Paint role for a hat segment, kept to theme tokens: the teal cone face
/// (`accent`), the cream "lit" face that gives the cone its logo-like 3D split
/// (`text`), and the gold tassel (`status_warn`).
#[derive(Clone, Copy)]
enum HatPaint {
    Cone,
    Lit,
    Tassel,
}
use HatPaint::{Cone, Lit, Tassel};

/// Block-art wizard hat placed to the left of the wordmark (echoes the app
/// icon): a solid quadrant-block cone (`▟`/`▙` sloped edges, `█` fill) split down
/// the middle into a teal face and a cream lit face — the logo's 3D two-tone —
/// with a gold tassel and a cream brim. Each row is a list of `(glyph, paint)`
/// segments; the teal/cream seam sits at a fixed column, and every row is the
/// same cell width so the wordmark starts at a fixed column per line.
const HAT_WIDTH_FULL: usize = 15;
const HAT_FULL: [&[(&str, HatPaint)]; 7] = [
    &[
        ("      ▟", Cone),
        ("▙", Lit),
        ("╮", Tassel),
        ("      ", Cone),
    ],
    &[
        ("     ▟█", Cone),
        ("█▙", Lit),
        ("┊", Tassel),
        ("     ", Cone),
    ],
    &[
        ("    ▟██", Cone),
        ("██▙", Lit),
        ("◦", Tassel),
        ("    ", Cone),
    ],
    &[("   ▟███", Cone), ("███▙    ", Lit)],
    &[("  ▟████", Cone), ("████▙   ", Lit)],
    &[(" ▟█████", Cone), ("█████▙  ", Lit)],
    &[("▗▄▄▄▄▄▄▄▄▄▄▄▄▖ ", Lit)],
];
const HAT_WIDTH_COMPACT: usize = 10;
const HAT_COMPACT: [&[(&str, HatPaint)]; 4] = [
    &[("   ▟", Cone), ("▙ ", Lit), ("╮", Tassel), ("   ", Cone)],
    &[("  ▟█", Cone), ("█▙", Lit), ("┊", Tassel), ("   ", Cone)],
    &[(" ▟██", Cone), ("██▙", Lit), ("◦", Tassel), ("  ", Cone)],
    &[("▗▄▄▄▄▄▄▄▖ ", Lit)],
];

/// A thin cream line drawn one row below the brim, in the otherwise-blank space,
/// reading as the brim's front rim so the brim looks like the logo's open
/// ellipse rather than a flat bar. Inset to sit under the brim's flat span.
const FULL_BRIM_RIM: &str = " ────────────";
const COMPACT_BRIM_RIM: &str = " ───────";

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
    /// The newest prior session ended non-terminally (task_13): swaps the static
    /// browser cue for a dynamic post-crash recovery nudge.
    pub recoverable_session: bool,
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
    // Trim the dead glyph rows, then drop the trailing blank columns each row
    // carries (the buffer is full chat width) so the hat can be prefixed without
    // pushing the line past `width` and wrapping.
    let word: Vec<Line<'static>> = trim_blank_edges(buffer_to_lines(&buffer))
        .into_iter()
        .map(right_trim_line)
        .collect();
    let full = matches!(pixel_size, PixelSize::Full);
    let hat = hat_lines(theme, full);
    let hat_width = if full {
        HAT_WIDTH_FULL
    } else {
        HAT_WIDTH_COMPACT
    };
    let mut lines = combine_hat_and_word(hat, word, hat_width);
    // The brim's front rim: a thin cream line in the blank row below the brim,
    // giving the brim the logo's open-ellipse look.
    let rim = if full {
        FULL_BRIM_RIM
    } else {
        COMPACT_BRIM_RIM
    };
    lines.push(Line::from(Span::styled(
        rim.to_string(),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    )));
    lines
}

/// Build the wizard-hat rows as styled lines: teal cone face (`accent`), cream
/// lit face (`text`), and gold tassel (`status_warn`), all matching the
/// wordmark's bold weight.
fn hat_lines(theme: &Theme, full: bool) -> Vec<Line<'static>> {
    let rows: &[&[(&str, HatPaint)]] = if full { &HAT_FULL } else { &HAT_COMPACT };
    rows.iter()
        .map(|row| {
            let spans = row
                .iter()
                .map(|(glyph, paint)| {
                    let color = match paint {
                        HatPaint::Cone => theme.accent,
                        HatPaint::Lit => theme.text,
                        HatPaint::Tassel => theme.status_warn,
                    };
                    Span::styled(
                        (*glyph).to_string(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// Place the hat to the left of the wordmark, bottom-aligned so the brim sits on
/// the wordmark baseline (a taller hat lets its tip rise above the letters).
/// Blank-padded rows keep `hat_width` cells so the wordmark stays column-aligned.
fn combine_hat_and_word(
    hat: Vec<Line<'static>>,
    word: Vec<Line<'static>>,
    hat_width: usize,
) -> Vec<Line<'static>> {
    let height = hat.len().max(word.len());
    let hat_pad = height - hat.len();
    let word_pad = height - word.len();
    (0..height)
        .map(|row| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if row >= hat_pad {
                spans.extend(hat[row - hat_pad].spans.clone());
            } else {
                spans.push(Span::raw(" ".repeat(hat_width)));
            }
            spans.push(Span::raw("  "));
            if row >= word_pad {
                spans.extend(word[row - word_pad].spans.clone());
            }
            Line::from(spans)
        })
        .collect()
}

/// Drop trailing whitespace spans (and trailing spaces on the last span) so a
/// wordmark row is only as wide as its glyphs.
fn right_trim_line(line: Line<'static>) -> Line<'static> {
    let mut spans = line.spans;
    while spans
        .last()
        .is_some_and(|span| span.content.chars().all(char::is_whitespace))
    {
        spans.pop();
    }
    if let Some(last) = spans.last_mut() {
        *last = Span::styled(last.content.trim_end().to_string(), last.style);
    }
    Line::from(spans)
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
    // Empty-state onboarding hint (task_08): the welcome only renders on an
    // empty chat, so this routing mental-model line is self-gating — no state or
    // events. Sits beside the existing `/help` cue.
    lines.push(Line::from(Span::styled(
        "describe a task — it routes through an orchestrator to named agents",
        Style::default().fg(theme.text_muted),
    )));
    lines.push(Line::from(Span::styled(
        "type /help for commands",
        Style::default().fg(theme.text_muted),
    )));
    // Browser discoverability cue (task_09) → dynamic post-crash hint (task_13):
    // when the newest prior session ended non-terminally, replace the neutral
    // cue with a recovery nudge in a warning tone; otherwise keep the muted cue.
    if facts.recoverable_session {
        lines.push(Line::from(Span::styled(
            "⚠ your last session was interrupted — Ctrl-R or /sessions to resume it",
            Style::default().fg(theme.status_warn),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Ctrl-R or /sessions to browse and resume past sessions",
            Style::default().fg(theme.text_muted),
        )));
    }
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
            recoverable_session: false,
        }
    }

    fn line_text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn hat_rows_have_consistent_cell_width() {
        // Every hat row must be the same width so the wordmark starts at a fixed
        // column on each line; a mismatch would jag the wordmark's left edge.
        let widths = |rows: &[&[(&str, HatPaint)]]| -> Vec<usize> {
            rows.iter()
                .map(|row| row.iter().map(|(g, _)| g.chars().count()).sum())
                .collect()
        };
        for w in widths(&HAT_FULL) {
            assert_eq!(w, HAT_WIDTH_FULL, "full hat row width");
        }
        for w in widths(&HAT_COMPACT) {
            assert_eq!(w, HAT_WIDTH_COMPACT, "compact hat row width");
        }
    }

    #[test]
    fn wordmark_includes_hat_at_full_and_compact() {
        let theme = truecolor();
        for width in [100, 70] {
            let text: String = wordmark_lines(&theme, width)
                .iter()
                .map(line_text)
                .collect();
            assert!(
                text.contains('▟') && text.contains('▙'),
                "solid cone edges missing at width {width}"
            );
            assert!(text.contains('▄'), "brim missing at width {width}");
            // Three hat tokens render: the gold tassel (status_warn) and the
            // cream lit face (text) are both distinct from the teal cone accent.
            let uses = |token| {
                wordmark_lines(&theme, width)
                    .iter()
                    .any(|line| line.spans.iter().any(|s| s.style.fg == Some(token)))
            };
            assert!(
                uses(theme.status_warn),
                "tassel gold token missing at {width}"
            );
            assert!(uses(theme.text), "cream lit-face token missing at {width}");
        }
    }

    #[test]
    fn width_ladder_selects_full_compact_or_plain() {
        let theme = truecolor();
        let full = wordmark_lines(&theme, 100);
        let compact = wordmark_lines(&theme, 70);
        // The hat (7 rows) dominates the trimmed wordmark height, plus one brim
        // rim row; compact is the 4-row quadrant block plus the rim. Full stays
        // taller than compact.
        assert_eq!(full.len(), 8);
        assert!(full.len() > compact.len());
        assert_eq!(compact.len(), COMPACT_HEIGHT as usize + 1);

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
    fn facts_box_includes_routing_hint_and_help_cue() {
        // The empty-state onboarding hint (task_08) must teach the routing
        // mental model and keep the existing `/help` cue beside it.
        let agents = [agent("explorer")];
        let lines = facts_lines(&truecolor(), &facts_with(&agents, None));
        let text: String = lines
            .iter()
            .map(|line| format!("{}\n", line_text(line)))
            .collect();

        assert!(
            text.contains("orchestrator"),
            "routing hint names the orchestrator"
        );
        assert!(
            text.contains("named agents"),
            "routing hint names the agents"
        );
        assert!(
            text.contains("type /help for commands"),
            "existing /help cue retained"
        );

        // The hint carries no inline color — it is styled with a theme token, so
        // every span's foreground is the shared muted token.
        let hint = lines
            .iter()
            .find(|line| line_text(line).contains("orchestrator"))
            .expect("hint line present");
        assert!(
            hint.spans
                .iter()
                .all(|span| span.style.fg == Some(truecolor().text_muted)),
            "hint styled with theme.text_muted token"
        );
    }

    #[test]
    fn facts_box_includes_session_browser_cue() {
        // task_09: a static cue points users at the session browser, styled with
        // the shared muted token (no inline color).
        let agents = [agent("explorer")];
        let lines = facts_lines(&truecolor(), &facts_with(&agents, None));
        let text: String = lines
            .iter()
            .map(|line| format!("{}\n", line_text(line)))
            .collect();
        assert!(text.contains("/sessions"), "session browser cue present");
        let cue = lines
            .iter()
            .find(|line| line_text(line).contains("/sessions"))
            .expect("cue line present");
        assert!(
            cue.spans
                .iter()
                .all(|span| span.style.fg == Some(truecolor().text_muted)),
            "cue styled with theme.text_muted token"
        );
    }

    #[test]
    fn facts_box_shows_dynamic_post_crash_hint_when_recoverable() {
        // task_13: when the newest prior session ended non-terminally, the static
        // cue is replaced by a recovery nudge in the warning tone.
        let agents = [agent("explorer")];
        let facts = WelcomeFacts {
            version: "1.0.0",
            working_directory: None,
            agents: &agents,
            preset: Some("default"),
            warnings: 0,
            git: None,
            recoverable_session: true,
        };
        let lines = facts_lines(&truecolor(), &facts);
        let text: String = lines
            .iter()
            .map(|line| format!("{}\n", line_text(line)))
            .collect();
        assert!(
            text.to_lowercase().contains("interrupted"),
            "post-crash hint shown: {text}"
        );
        assert!(
            text.contains("Ctrl-R") || text.contains("/sessions"),
            "the hint points at recovery: {text}"
        );
        assert!(
            !text.contains("browse and resume past sessions"),
            "the static cue is replaced, not shown alongside the dynamic hint"
        );
        let hint = lines
            .iter()
            .find(|line| line_text(line).to_lowercase().contains("interrupted"))
            .expect("hint line present");
        assert!(
            hint.spans
                .iter()
                .all(|span| span.style.fg == Some(truecolor().status_warn)),
            "post-crash hint uses the warning theme token"
        );
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
