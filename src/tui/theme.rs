use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders};

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,           // main background
    pub fg: Color,           // main foreground text
    pub fg_dim: Color,       // dim text (secondary info)
    pub accent: Color,       // strong accent (focus border, PC line)
    pub accent_soft: Color,  // weaker accent (selection, highlights)
    pub border: Color,       // normal border
    pub border_dim: Color,   // unfocused border
    pub status_bg: Color,    // header background
    pub status_fg: Color,    // header foreground
    pub cmdline_bg: Color,   // command line background
    pub cmdline_fg: Color,   // command line foreground
    pub popup_bg: Color,     // symbols popup background
    pub popup_border: Color, // symbols popup border color
    pub error: Color,        // error text

    // Panel card styling
    pub panel_bg: Color,     // panel background (floating card effect)
    pub panel_shadow: Color, // panel shadow color
    pub separator: Color,    // separator line color

    // VM region colors
    pub vm_stack: Color,
    pub vm_heap: Color,
    pub vm_data: Color,
    pub vm_text: Color,
}

pub const THEME_DARK: Theme = Theme {
    bg: Color::Rgb(11, 14, 20),
    fg: Color::Rgb(210, 210, 210),
    fg_dim: Color::Rgb(140, 140, 150),
    accent: Color::Cyan,
    accent_soft: Color::Rgb(70, 140, 170),
    border: Color::Rgb(80, 80, 100),
    border_dim: Color::Rgb(45, 45, 60),
    status_bg: Color::Rgb(20, 23, 40),
    status_fg: Color::Rgb(230, 230, 230),
    cmdline_bg: Color::Rgb(15, 18, 32),
    cmdline_fg: Color::Rgb(210, 210, 210),
    popup_bg: Color::Rgb(18, 21, 32),
    popup_border: Color::Rgb(120, 200, 255),
    error: Color::Red,

    panel_bg: Color::Rgb(18, 21, 32),
    panel_shadow: Color::Rgb(8, 10, 16),
    separator: Color::Rgb(60, 90, 120),

    vm_stack: Color::Green,
    vm_heap: Color::Cyan,
    vm_data: Color::Yellow,
    vm_text: Color::Magenta,
};

impl Theme {
    pub fn default() -> Self {
        THEME_DARK
    }
}

pub fn theme() -> &'static Theme {
    &THEME_DARK
}

/// Create a styled panel block with focus-dependent styling (floating card style)
pub fn panel_block<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    let border_color = if focused {
        theme.accent
    } else {
        theme.border_dim
    };

    let base_style = Style::default().bg(theme.panel_bg).fg(theme.fg);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(base_style)
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
}

/// Create a styled block for the symbols popup (same card style as panels)
pub fn symbols_popup_block<'a>(focused: bool, theme: &Theme) -> Block<'a> {
    let border_color = if focused {
        theme.popup_border
    } else {
        theme.border_dim
    };

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " Symbols ",
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.panel_bg))
}
