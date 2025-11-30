use crate::tui::theme::{self, Theme};
use ratatui::{prelude::*, text::{Line, Span}, widgets::{Clear, Paragraph, Wrap}};

/// Render VM panel with colored region labels
pub fn render_vm_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    lines: &[String],
    scroll_y: u16,
) {
    // Clear the panel area first to avoid stale characters after resize.
    f.render_widget(Clear, area);

    // Process lines to add colors for VM regions
    let mut styled_lines: Vec<Line> = Vec::new();
    for line_str in lines {
        let line_lower = line_str.to_lowercase();

        let styled_line = if line_lower.contains("[stack]") {
            Line::from(vec![
                Span::styled("▉▉▉ ", Style::default().fg(theme.vm_stack)),
                Span::styled(line_str.clone(), Style::default().fg(theme.fg)),
            ])
        } else if line_lower.contains("[heap]") {
            Line::from(vec![
                Span::styled("▉▉▉ ", Style::default().fg(theme.vm_heap)),
                Span::styled(line_str.clone(), Style::default().fg(theme.fg)),
            ])
        } else if line_lower.contains("[data]") {
            Line::from(vec![
                Span::styled("▉▉▉ ", Style::default().fg(theme.vm_data)),
                Span::styled(line_str.clone(), Style::default().fg(theme.fg)),
            ])
        } else if line_lower.contains("[text]") {
            Line::from(vec![
                Span::styled("▉▉▉ ", Style::default().fg(theme.vm_text)),
                Span::styled(line_str.clone(), Style::default().fg(theme.fg)),
            ])
        } else if line_lower.contains("addr") {
            Line::from(Span::styled(
                line_str.clone(),
                Style::default().fg(theme.fg_dim),
            ))
        } else {
            Line::from(line_str.clone())
        };

        styled_lines.push(styled_line);
    }

    let block = theme::panel_block(" VM Layout ", focused, theme);
    let para = Paragraph::new(styled_lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
    f.render_widget(para, area);
}
