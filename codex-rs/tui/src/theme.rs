//! Browser Use terminal palette: Catppuccin Mocha (dark) / Latte (light).
//!
//! Ported from browser-use/terminal `crates/browser-use-tui/src/theme.rs` so the
//! embedded agent TUI matches the Browser Use Terminal look. Variant selection
//! honours `BH_THEME`/`BUT_THEME` (`light`/`dark`), otherwise it follows the
//! terminal-background probe already used by the rest of the UI chrome.

use crate::color::is_light as bg_is_light;
use crate::terminal_palette::best_color;
use crate::terminal_palette::default_bg;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Variant {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub variant: Variant,
    pub text: (u8, u8, u8),
    pub muted: (u8, u8, u8),
    pub dim: (u8, u8, u8),
    pub accent: (u8, u8, u8),
    pub link: (u8, u8, u8),
    pub path_reference: (u8, u8, u8),
    pub code: (u8, u8, u8),
    pub code_background: (u8, u8, u8),
    pub code_block_fg: (u8, u8, u8),
    pub heading: (u8, u8, u8),
    pub quote: (u8, u8, u8),
    pub border: (u8, u8, u8),
    pub done: (u8, u8, u8),
    pub running: (u8, u8, u8),
    pub failed: (u8, u8, u8),
    pub thought: (u8, u8, u8),
    pub user_prompt_background: (u8, u8, u8),
    pub activity_group: (u8, u8, u8),
    pub activity_read: (u8, u8, u8),
    pub activity_run: (u8, u8, u8),
    pub activity_list: (u8, u8, u8),
    pub activity_search: (u8, u8, u8),
    pub activity_task: (u8, u8, u8),
    pub selection_background: (u8, u8, u8),
}

impl Palette {
    /// Catppuccin Mocha — dark.
    pub(crate) const fn mocha() -> Self {
        Self {
            variant: Variant::Dark,
            text: (205, 214, 244),
            muted: (166, 173, 200),
            dim: (108, 112, 134),
            accent: (137, 180, 250),
            link: (137, 220, 235),
            path_reference: (250, 179, 135),
            code: (180, 190, 254),
            code_background: (49, 50, 68),
            code_block_fg: (186, 194, 222),
            heading: (250, 179, 135),
            quote: (147, 153, 178),
            border: (69, 71, 90),
            done: (166, 227, 161),
            running: (250, 179, 135),
            failed: (243, 139, 168),
            thought: (203, 166, 247),
            user_prompt_background: (49, 50, 68),
            activity_group: (166, 227, 161),
            activity_read: (137, 180, 250),
            activity_run: (250, 179, 135),
            activity_list: (148, 226, 213),
            activity_search: (249, 226, 175),
            activity_task: (180, 190, 254),
            selection_background: (45, 52, 66),
        }
    }

    /// Catppuccin Latte — light.
    pub(crate) const fn latte() -> Self {
        Self {
            variant: Variant::Light,
            text: (76, 79, 105),
            muted: (108, 111, 133),
            dim: (156, 160, 176),
            accent: (30, 102, 245),
            link: (4, 165, 229),
            path_reference: (254, 100, 11),
            code: (114, 135, 253),
            code_background: (230, 233, 239),
            code_block_fg: (92, 95, 119),
            heading: (254, 100, 11),
            quote: (140, 143, 161),
            border: (204, 208, 218),
            done: (64, 160, 43),
            running: (254, 100, 11),
            failed: (210, 15, 57),
            thought: (136, 57, 239),
            user_prompt_background: (230, 233, 239),
            activity_group: (64, 160, 43),
            activity_read: (30, 102, 245),
            activity_run: (254, 100, 11),
            activity_list: (23, 146, 153),
            activity_search: (223, 142, 29),
            activity_task: (114, 135, 253),
            selection_background: (220, 224, 232),
        }
    }
}

pub(crate) fn palette() -> Palette {
    palette_for(default_bg())
}

/// Palette for an explicit terminal background; `BH_THEME`/`BUT_THEME` wins.
pub(crate) fn palette_for(terminal_bg: Option<(u8, u8, u8)>) -> Palette {
    match std::env::var("BH_THEME")
        .or_else(|_| std::env::var("BUT_THEME"))
        .ok()
        .as_deref()
    {
        Some("light") | Some("LIGHT") => return Palette::latte(),
        Some("dark") | Some("DARK") => return Palette::mocha(),
        _ => {}
    }
    if terminal_bg.is_some_and(bg_is_light) {
        Palette::latte()
    } else {
        Palette::mocha()
    }
}

pub(crate) fn is_light() -> bool {
    palette().variant == Variant::Light
}

/// Quantize a palette RGB to the terminal's color capability.
pub(crate) fn color(rgb: (u8, u8, u8)) -> Color {
    best_color(rgb)
}

pub(crate) fn text() -> Style {
    Style::default().fg(color(palette().text))
}

pub(crate) fn bold() -> Style {
    text().add_modifier(Modifier::BOLD)
}

pub(crate) fn muted() -> Style {
    Style::default().fg(color(palette().muted))
}

pub(crate) fn dim() -> Style {
    Style::default().fg(color(palette().dim))
}

pub(crate) fn accent() -> Style {
    Style::default()
        .fg(color(palette().accent))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn border() -> Style {
    Style::default().fg(color(palette().border))
}

pub(crate) fn link() -> Style {
    Style::default()
        .fg(color(palette().link))
        .add_modifier(Modifier::UNDERLINED)
}

/// Background fill for a user prompt block in the transcript, so the message
/// the user sent stands apart from the agent's replies.
pub(crate) fn user_prompt_bg() -> Color {
    color(palette().user_prompt_background)
}

pub(crate) fn user_prompt_text() -> Style {
    text().bg(user_prompt_bg())
}

/// The accent-colored `>` prefix on a user prompt, sharing the prompt's
/// highlight background.
pub(crate) fn user_prompt_accent() -> Style {
    accent().bg(user_prompt_bg())
}

pub(crate) fn done() -> Style {
    Style::default().fg(color(palette().done))
}

/// Style for the list item currently in use (the active model/provider),
/// distinct from the cursor highlight so the active choice stands out.
pub(crate) fn current() -> Style {
    done().add_modifier(Modifier::BOLD)
}

pub(crate) fn running() -> Style {
    Style::default().fg(color(palette().running))
}

pub(crate) fn failed() -> Style {
    Style::default().fg(color(palette().failed))
}

pub(crate) fn thought() -> Style {
    Style::default().fg(color(palette().thought))
}

pub(crate) fn selection() -> Style {
    Style::default().bg(color(palette().selection_background))
}
