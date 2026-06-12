//! Single source of color truth for the TUI.
//!
//! Every semantic token derives from the web brand palette
//! (`web/src/styles/global.css`, ADR-003) so TUI screenshots and the website
//! read as one product. Each token carries a truecolor RGB value, a
//! hand-picked ANSI-256 fallback, and a monochrome treatment.
//! [`Theme::resolve`] picks exactly one tier per token at startup from the
//! detected [`TerminalCaps`] (ADR-004), so render code never branches on
//! terminal capabilities — it just reads the resolved [`Color`].
//!
//! The struct is serde-ready (a flat record of [`Color`] values; ratatui's
//! `serde` feature is enabled) so a future user-facing theme file (V2) can
//! load into it without reshaping.

use std::env;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Number of round-robin agent accent colors. Excludes `status_error` red so
/// an agent accent is never mistaken for an error (ADR-006).
pub const AGENT_ACCENT_COUNT: usize = 5;

/// Detected terminal color capabilities. Pure data — see [`TerminalCaps::detect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCaps {
    /// `NO_COLOR` is set to any non-empty value (<https://no-color.org/>).
    pub no_color: bool,
    /// `COLORTERM` advertises truecolor (`truecolor` or `24bit`).
    pub truecolor: bool,
}

impl TerminalCaps {
    /// Detect capabilities from the process environment. The only place in the
    /// codebase that reads `NO_COLOR`/`COLORTERM` (requirement: nothing else may).
    pub fn detect() -> Self {
        Self::from_env(read_env("NO_COLOR"), read_env("COLORTERM"))
    }

    /// Resolve capabilities from raw env values. Pure function — injectable so
    /// tests exercise every tier without touching the process environment.
    pub fn from_env(no_color: Option<String>, colorterm: Option<String>) -> Self {
        let no_color = no_color.is_some_and(|value| !value.is_empty());
        let truecolor = colorterm.is_some_and(|value| {
            let value = value.trim();
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        });
        Self {
            no_color,
            truecolor,
        }
    }
}

fn read_env(key: &str) -> Option<String> {
    env::var_os(key).map(|value| value.to_string_lossy().into_owned())
}

/// One token's value across the three capability tiers. The mono tier is
/// always `Color::Reset` (the terminal's own foreground/background) so a
/// `NO_COLOR` session emits no brand color at all.
#[derive(Clone, Copy, Debug)]
struct TokenSpec {
    rgb: Color,
    ansi256: Color,
}

impl TokenSpec {
    const fn new(r: u8, g: u8, b: u8, ansi256: u8) -> Self {
        Self {
            rgb: Color::Rgb(r, g, b),
            ansi256: Color::Indexed(ansi256),
        }
    }

    fn pick(self, caps: TerminalCaps) -> Color {
        if caps.no_color {
            Color::Reset
        } else if caps.truecolor {
            self.rgb
        } else {
            self.ansi256
        }
    }
}

// ── Web-palette tokens: (truecolor RGB from global.css, hand-picked ANSI-256) ──
const TEXT: TokenSpec = TokenSpec::new(0xf2, 0xea, 0xd8, 230); // --text
const TEXT_MUTED: TokenSpec = TokenSpec::new(0xb7, 0xae, 0x9e, 144); // --muted
const TEXT_DIM: TokenSpec = TokenSpec::new(0x7e, 0x86, 0x7a, 102); // --dim
const BORDER: TokenSpec = TokenSpec::new(0x2c, 0x34, 0x2d, 236); // --line
const BORDER_FOCUSED: TokenSpec = TokenSpec::new(0x43, 0x50, 0x44, 239); // --line-strong
const ACCENT: TokenSpec = TokenSpec::new(0x79, 0xe2, 0xe1, 80); // --cyan (brand accent)
const STATUS_OK: TokenSpec = TokenSpec::new(0x8c, 0xff, 0xb0, 121); // --green
const STATUS_WARN: TokenSpec = TokenSpec::new(0xff, 0xb4, 0x54, 215); // --amber
const STATUS_ERROR: TokenSpec = TokenSpec::new(0xff, 0x6f, 0x61, 203); // --red

// Near-black brand ink (--ink). Used as the dark foreground on bright badges
// and selection highlights, mirroring the web `.brand__mark` (ink-on-green).
const INK: TokenSpec = TokenSpec::new(0x06, 0x08, 0x07, 232);

// Subtle cyan-family tint behind a submitted user prompt. Absorbs the legacy
// `USER_EVENT_BG` (`Color::Rgb(18, 52, 71)`); recolored to the brand cyan family.
const USER_PROMPT_BG: TokenSpec = TokenSpec::new(0x12, 0x2b, 0x2b, 23);

// Round-robin agent accents (ADR-006). The palette ships three non-red accents
// (cyan/amber/green); two harmonious hues (lavender, verdigris-teal) extend the
// pool to the TechSpec's `[Color; 5]` while staying distinct from each neighbour
// and from `STATUS_ERROR`. Ordered for adjacent contrast.
const AGENT_ACCENTS: [TokenSpec; AGENT_ACCENT_COUNT] = [
    TokenSpec::new(0x79, 0xe2, 0xe1, 80),  // cyan
    TokenSpec::new(0xff, 0xb4, 0x54, 215), // amber
    TokenSpec::new(0x8c, 0xff, 0xb0, 121), // green
    TokenSpec::new(0xb8, 0xa6, 0xe8, 147), // lavender
    TokenSpec::new(0x5c, 0xc6, 0xb8, 79),  // verdigris-teal
];

/// Resolved color tokens for the whole TUI. Built once at startup via
/// [`Theme::resolve`] and threaded through render code via `TuiUiState`
/// (ADR-004); render helpers read these fields and never inspect capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub text: Color,
    pub text_muted: Color,
    pub text_dim: Color,
    pub border: Color,
    pub border_focused: Color,
    pub accent: Color,
    pub status_ok: Color,
    pub status_warn: Color,
    pub status_error: Color,
    /// Near-black ink for dark foregrounds on bright badges/selections.
    pub ink: Color,
    pub user_prompt_bg: Color,
    pub agent_accents: [Color; AGENT_ACCENT_COUNT],
}

impl Theme {
    /// Resolve every token against detected capabilities: RGB under truecolor,
    /// hand-picked `Color::Indexed` under 256-color, `Color::Reset` (terminal
    /// default, no brand color) under `NO_COLOR`.
    pub fn resolve(caps: TerminalCaps) -> Theme {
        Theme {
            text: TEXT.pick(caps),
            text_muted: TEXT_MUTED.pick(caps),
            text_dim: TEXT_DIM.pick(caps),
            border: BORDER.pick(caps),
            border_focused: BORDER_FOCUSED.pick(caps),
            accent: ACCENT.pick(caps),
            status_ok: STATUS_OK.pick(caps),
            status_warn: STATUS_WARN.pick(caps),
            status_error: STATUS_ERROR.pick(caps),
            ink: INK.pick(caps),
            user_prompt_bg: USER_PROMPT_BG.pick(caps),
            agent_accents: AGENT_ACCENTS.map(|spec| spec.pick(caps)),
        }
    }

    /// Pick the accent for an agent by its roster index, cycling the pool
    /// deterministically (ADR-006). Adjacent indices are distinct up to the
    /// pool length; `accent_for(i) == accent_for(i + pool_len)`.
    pub fn accent_for(&self, agent_index: usize) -> Color {
        self.agent_accents[agent_index % self.agent_accents.len()]
    }

    /// True when resolved under `NO_COLOR`: every token is the terminal default,
    /// so brand-color-dependent flourishes (e.g. the wordmark) are skipped.
    pub fn is_monochrome(&self) -> bool {
        self.text == Color::Reset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    fn truecolor() -> Theme {
        Theme::resolve(TerminalCaps {
            no_color: false,
            truecolor: true,
        })
    }

    /// Every `Color` field of a resolved theme, for tier-wide assertions.
    fn all_colors(theme: &Theme) -> Vec<Color> {
        let mut colors = vec![
            theme.text,
            theme.text_muted,
            theme.text_dim,
            theme.border,
            theme.border_focused,
            theme.accent,
            theme.status_ok,
            theme.status_warn,
            theme.status_error,
            theme.ink,
            theme.user_prompt_bg,
        ];
        colors.extend(theme.agent_accents);
        colors
    }

    #[test]
    fn caps_truecolor_without_no_color() {
        let caps = TerminalCaps::from_env(Some(String::new()), Some("truecolor".to_string()));
        assert!(caps.truecolor);
        assert!(!caps.no_color);

        let unset = TerminalCaps::from_env(None, Some("24bit".to_string()));
        assert!(unset.truecolor);
        assert!(!unset.no_color);
    }

    #[test]
    fn caps_no_color_overrides_colorterm() {
        let caps = TerminalCaps::from_env(Some("1".to_string()), Some("truecolor".to_string()));
        assert!(caps.no_color);
    }

    #[test]
    fn resolve_truecolor_uses_web_hex() {
        let theme = truecolor();
        assert_eq!(theme.text, Color::Rgb(0xf2, 0xea, 0xd8));
        assert_eq!(theme.accent, Color::Rgb(0x79, 0xe2, 0xe1));
        assert_eq!(theme.status_warn, Color::Rgb(0xff, 0xb4, 0x54));
    }

    #[test]
    fn resolve_256_color_indexes_every_token() {
        let theme = Theme::resolve(TerminalCaps {
            no_color: false,
            truecolor: false,
        });
        for color in all_colors(&theme) {
            assert!(
                matches!(color, Color::Indexed(_)),
                "expected Indexed under 256-color, got {color:?}"
            );
        }
    }

    #[test]
    fn resolve_no_color_emits_no_brand_color() {
        let theme = Theme::resolve(TerminalCaps {
            no_color: true,
            truecolor: true,
        });
        for color in all_colors(&theme) {
            assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                "NO_COLOR must not emit a brand color, got {color:?}"
            );
            assert_eq!(color, Color::Reset);
        }
    }

    #[test]
    fn accent_for_cycles_pool() {
        let theme = truecolor();
        let pool = theme.agent_accents.len();
        for index in 0..(2 * pool) {
            assert_ne!(
                theme.accent_for(index),
                theme.accent_for(index + 1),
                "adjacent accents must differ at index {index}"
            );
            assert_eq!(
                theme.accent_for(index),
                theme.accent_for(index + pool),
                "accent must repeat every pool length at index {index}"
            );
        }
    }

    #[test]
    fn agent_accents_exclude_error_red() {
        let theme = truecolor();
        let red = theme.status_error;
        assert!(
            !theme.agent_accents.contains(&red),
            "error red must not be an agent accent"
        );
    }

    #[test]
    fn resolved_theme_styles_a_render_without_panicking() {
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        let theme = truecolor();
        terminal
            .draw(|frame| {
                let paragraph = Paragraph::new("atelier")
                    .style(Style::default().fg(theme.text).bg(theme.user_prompt_bg));
                frame.render_widget(paragraph, frame.area());
            })
            .unwrap();
    }
}
