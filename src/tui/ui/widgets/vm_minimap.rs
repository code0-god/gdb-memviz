use crate::tui::theme::Theme;
use crate::vm::{VmLabel, VmLayout};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Widget},
};

/// VM minimap widget that visualizes memory regions as a vertical bar
pub struct VmMinimap<'a> {
    pub layout: &'a VmLayout,
    pub cursor_addr: Option<u64>,
    pub theme: &'a Theme,
}

impl<'a> Widget for VmMinimap<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Handle edge cases: zero-size area or empty layout
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Draw a border around the minimap
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.theme.border_dim))
            .style(Style::default().bg(self.theme.panel_bg));

        let inner = block.inner(area);
        block.render(area, buf);

        // If there's no inner space after borders, return early
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Get the overall address range
        let (vm_min, vm_max) = match self.layout.addr_range() {
            Some(range) => range,
            None => {
                // No regions, render a placeholder message
                self.render_empty_state(inner, buf);
                return;
            }
        };

        let total = vm_max.saturating_sub(vm_min);
        if total == 0 {
            return;
        }

        // Determine if any region contains the cursor
        let cursor_region = self
            .cursor_addr
            .and_then(|addr| self.layout.region_at(addr));

        // Render each region as a vertical slice
        for region in &self.layout.regions {
            // Calculate the vertical position of this region
            let start_rel = (region.start.saturating_sub(vm_min)) as f64 / total as f64;
            let end_rel = (region.end.saturating_sub(vm_min)) as f64 / total as f64;

            let y0 = (start_rel * inner.height as f64).floor() as u16;
            let y1 = (end_rel * inner.height as f64).ceil() as u16;

            // Clamp to inner area height
            let y0 = y0.min(inner.height.saturating_sub(1));
            let y1 = y1.min(inner.height);

            // Get the color for this region type
            let bg_color = self.region_color(&region.label);

            // Check if this region should be highlighted (contains cursor)
            let is_cursor_region = cursor_region
                .map(|cr| std::ptr::eq(cr, region))
                .unwrap_or(false);

            let style = if is_cursor_region {
                // Highlight the cursor region with bold modifier and brighter color
                Style::default()
                    .bg(bg_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(bg_color)
            };

            // Fill the region's vertical slice
            for y in y0..y1 {
                for x in 0..inner.width {
                    let cell_x = inner.x + x;
                    let cell_y = inner.y + y;

                    // Ensure we're within buffer bounds
                    if cell_x < buf.area.right() && cell_y < buf.area.bottom() {
                        let cell = buf.get_mut(cell_x, cell_y);
                        cell.set_style(style);
                        cell.set_char(' '); // Fill with space to show background color
                    }
                }
            }
        }
    }
}

impl<'a> VmMinimap<'a> {
    /// Get the background color for a given VM region label
    fn region_color(&self, label: &VmLabel) -> Color {
        match label {
            VmLabel::Text => self.theme.vm_text,
            VmLabel::Data => self.theme.vm_data,
            VmLabel::Heap => self.theme.vm_heap,
            VmLabel::Stack => self.theme.vm_stack,
            VmLabel::Lib => self.theme.fg_dim,
            VmLabel::Anonymous => self.theme.border_dim,
            VmLabel::Other(_) => self.theme.border,
        }
    }

    /// Render a message when the layout is empty
    fn render_empty_state(&self, area: Rect, buf: &mut Buffer) {
        let msg = "No VM data";
        if area.width >= msg.len() as u16 && area.height > 0 {
            let x = area.x + (area.width.saturating_sub(msg.len() as u16)) / 2;
            let y = area.y + area.height / 2;

            for (i, ch) in msg.chars().enumerate() {
                let cell_x = x + i as u16;
                if cell_x < buf.area.right() && y < buf.area.bottom() {
                    let cell = buf.get_mut(cell_x, y);
                    cell.set_char(ch);
                    cell.set_style(Style::default().fg(self.theme.fg_dim));
                }
            }
        }
    }
}
