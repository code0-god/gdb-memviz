use crate::tui::theme::Theme;
use ratatui::{prelude::*, text::{Line, Span}, widgets::Paragraph};

/// Render command line in Neovim-style
pub fn render_cmdline(f: &mut Frame, theme: &Theme, area: Rect) {
    let line = Line::from(vec![
        Span::styled(":", Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled("(future command mode)", Style::default().fg(theme.fg_dim)),
    ]);

    let cmd =
        Paragraph::new(line).style(Style::default().bg(theme.cmdline_bg).fg(theme.cmdline_fg));
    f.render_widget(cmd, area);
}
