use crate::tui::theme::{self, Theme};
use crate::tui::ui::widgets::VmMinimap;
use crate::vm::VmLayout;
use ratatui::{prelude::*, text::{Line, Span}, widgets::{Clear, Paragraph, Wrap}};

/// Render VM panel with colored region labels and minimap
pub fn render_vm_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    lines: &[String],
    scroll_y: u16,
    vm_layout: &VmLayout,
    cursor_addr: Option<u64>,
) {
    // Clear the panel area first to avoid stale characters after resize.
    f.render_widget(Clear, area);

    // Create and render the panel block
    let block = theme::panel_block(" VM Layout ", focused, theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

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

    // Render the text paragraph in the inner area
    let para = Paragraph::new(styled_lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg))
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
    f.render_widget(para, inner);

    // Render the minimap in the top-right corner of the inner area
    let minimap_width = inner.width.min(18);

    // Calculate minimap height based on band configuration
    // Each unit = 1 row, so total rows = sum of all band units
    use crate::tui::ui::widgets::vm_minimap::VmBandLayoutConfig;
    let band_config = VmBandLayoutConfig::default();
    let total_units = band_config.stack + band_config.unalloc1 + band_config.lib
        + band_config.unalloc2 + band_config.heap + band_config.data + band_config.text;

    // Ideal height = total_units (for bands) + 2 (for borders)
    let ideal_minimap_height = total_units + 2;
    let minimap_height = ideal_minimap_height.min(inner.height);

    if minimap_width >= 8 && minimap_height >= 7 {
        let minimap_area = Rect {
            x: inner.x + inner.width.saturating_sub(minimap_width),
            y: inner.y,
            width: minimap_width,
            height: minimap_height,
        };

        let minimap = VmMinimap::new(vm_layout, cursor_addr, theme);

        // Render the minimap after the text, so it overlays the text area
        f.render_widget(minimap, minimap_area);
    }
}
