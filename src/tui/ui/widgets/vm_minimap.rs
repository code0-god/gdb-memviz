use crate::tui::theme::Theme;
use crate::vm::{VmBand, VmBandKind, VmLayout};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Widget},
};

/// Relative heights (in "units") for each conceptual band.
#[derive(Debug, Clone, Copy)]
pub struct VmBandLayoutConfig {
    pub stack: u16,
    pub unalloc1: u16,
    pub lib: u16,
    pub unalloc2: u16,
    pub heap: u16,
    pub data: u16,
    pub text: u16,
}

impl Default for VmBandLayoutConfig {
    fn default() -> Self {
        Self {
            stack: 1,
            unalloc1: 1,
            lib: 1,
            unalloc2: 1,
            heap: 3,
            data: 1,
            text: 1,
        }
    }
}

/// VM minimap widget that visualizes memory regions as a canonical band layout
pub struct VmMinimap<'a> {
    pub layout: &'a VmLayout,
    pub cursor_addr: Option<u64>,
    pub theme: &'a Theme,
    pub band_config: VmBandLayoutConfig,
    pub border_color: Color,
}

impl<'a> VmMinimap<'a> {
    /// Create a new VmMinimap with default band configuration
    pub fn new(
        layout: &'a VmLayout,
        cursor_addr: Option<u64>,
        theme: &'a Theme,
        border_color: Color,
    ) -> Self {
        Self {
            layout,
            cursor_addr,
            theme,
            band_config: VmBandLayoutConfig::default(),
            border_color,
        }
    }
}

impl<'a> Widget for VmMinimap<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Handle edge cases: zero-size area
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Draw a border around the minimap
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.border_color))
            .style(Style::default().bg(self.theme.panel_bg));

        let inner = block.inner(area);
        block.render(area, buf);

        // If there's no inner space after borders, return early
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Get conceptual bands
        let bands = self.layout.bands();
        if bands.is_empty() {
            // No regions, render a placeholder message
            self.render_empty_state(inner, buf);
            return;
        }

        // Map from kind → &VmBand for quick lookup
        use std::collections::HashMap;
        let mut band_map = HashMap::new();
        for band in &bands {
            band_map.insert(band.kind, band);
        }

        // Collect (band_kind, units) in fixed high→low order
        let cfg = self.band_config;
        let band_units: &[(VmBandKind, u16)] = &[
            (VmBandKind::Stack, cfg.stack),
            (VmBandKind::Unallocated1, cfg.unalloc1),
            (VmBandKind::Lib, cfg.lib),
            (VmBandKind::Unallocated2, cfg.unalloc2),
            (VmBandKind::Heap, cfg.heap),
            (VmBandKind::Data, cfg.data),
            (VmBandKind::Text, cfg.text),
        ];

        let total_units: u16 = band_units.iter().map(|(_, u)| *u).sum();
        if total_units == 0 {
            return;
        }

        let h = inner.height;
        if h == 0 {
            return;
        }

        let mut used_rows: u16 = 0;
        let mut remaining_rows = h;

        for (idx, (kind, units)) in band_units.iter().enumerate() {
            // Get the band or create a placeholder with no span
            let placeholder = VmBand {
                kind: *kind,
                span: None,
            };
            let band = band_map.get(kind).copied().unwrap_or(&placeholder);

            // Compute this band's height in rows based on units
            let target = ((*units as u32) * (h as u32) / (total_units as u32)) as u16;
            let mut band_h = target.max(1);

            // Clamp to remaining rows
            if band_h > remaining_rows {
                band_h = remaining_rows;
            }

            // For the last band, only use remaining rows if it's within 1 of target
            // This prevents the last band from expanding too much
            if idx == band_units.len() - 1 && remaining_rows > 0 {
                // If there's a small rounding error (1-2 rows), give it to the last band
                if remaining_rows <= 2 || remaining_rows.abs_diff(target) <= 1 {
                    band_h = remaining_rows;
                }
            }

            let y0 = inner.y + used_rows;
            let y1 = y0.saturating_add(band_h).min(inner.y + h);
            if y0 >= y1 {
                break;
            }

            let is_primary = matches!(
                band.kind,
                VmBandKind::Stack | VmBandKind::Heap | VmBandKind::Data | VmBandKind::Text
            );

            self.draw_band(inner, buf, band, y0, y1, is_primary);

            used_rows += band_h;
            remaining_rows = h.saturating_sub(used_rows);
            if remaining_rows == 0 {
                break;
            }
        }

        // Draw address range labels outside the minimap bands
        let label_style = Style::default().fg(self.theme.fg_dim);

        // "high address" at top-left (above Stack band)
        let high_label = "high address";
        if area.y < buf.area.bottom() && area.x < buf.area.right() {
            for (i, ch) in high_label.chars().enumerate() {
                let x = area.x + i as u16;
                if x >= buf.area.right() {
                    break;
                }
                let cell = buf.get_mut(x, area.y);
                cell.set_symbol(&ch.to_string());
                cell.set_style(label_style);
            }
        }

        // "low address" at bottom-left (below Text band)
        let low_label = "low address";
        let bottom_y = area.y + area.height.saturating_sub(1);
        if bottom_y < buf.area.bottom() && area.x < buf.area.right() {
            for (i, ch) in low_label.chars().enumerate() {
                let x = area.x + i as u16;
                if x >= buf.area.right() {
                    break;
                }
                let cell = buf.get_mut(x, bottom_y);
                cell.set_symbol(&ch.to_string());
                cell.set_style(label_style);
            }
        }
    }
}

impl<'a> VmMinimap<'a> {
    /// Draw a single band as a vertical slice in the minimap
    fn draw_band(
        &self,
        inner: Rect,
        buf: &mut Buffer,
        band: &VmBand,
        y_start: u16,
        y_end: u16,
        is_primary: bool,
    ) {
        if y_start >= y_end || inner.width == 0 {
            return;
        }

        // Base color by band kind
        let mut style = match band.kind {
            VmBandKind::Stack => Style::default().bg(self.theme.vm_stack),
            VmBandKind::Heap => Style::default().bg(self.theme.vm_heap),
            VmBandKind::Data => Style::default().bg(self.theme.vm_data),
            VmBandKind::Text => Style::default().bg(self.theme.vm_text),
            VmBandKind::Lib => Style::default().bg(self.theme.vm_lib),
            VmBandKind::Unallocated1 | VmBandKind::Unallocated2 => {
                Style::default().bg(self.theme.vm_gap)
            }
        };

        // Dim non-primary bands if desired
        if !is_primary {
            style = style.fg(self.theme.fg_dim);
        }

        // Cursor highlight: bold if cursor_addr lies within band's span
        if let (Some(addr), Some((start, end))) = (self.cursor_addr, band.span) {
            if addr >= start && addr < end {
                style = style.add_modifier(Modifier::BOLD);
            }
        }

        // Fill the band's vertical slice
        for y in y_start..y_end {
            for x in 0..inner.width {
                let cell_x = inner.x + x;
                let cell_y = y;

                // Ensure we're within buffer bounds
                if cell_x < buf.area.right() && cell_y < buf.area.bottom() {
                    let cell = buf.get_mut(cell_x, cell_y);
                    cell.set_char(' '); // Fill with space to show background color
                    cell.set_style(style);
                }
            }
        }

        // Draw centered text label for the band
        let label = match band.kind {
            VmBandKind::Stack => "Stack",
            VmBandKind::Unallocated1 => "unallocated",
            VmBandKind::Lib => "Lib",
            VmBandKind::Unallocated2 => "unallocated",
            VmBandKind::Heap => "Heap",
            VmBandKind::Data => "Data",
            VmBandKind::Text => "Text",
        };

        let band_height = y_end.saturating_sub(y_start);
        if band_height > 0 && inner.width > 0 && !label.is_empty() {
            // Choose vertical position: upper middle of the band
            // For even heights, this chooses the upper of the two middle rows
            let label_y = y_start + (band_height - 1) / 2;

            // Center horizontally if possible
            let label_width = label.chars().count() as u16;
            let label_x = if label_width + 2 <= inner.width {
                // center horizontally within the band area
                inner.x + (inner.width - label_width) / 2
            } else {
                // if too narrow, align to the left
                inner.x
            };

            // Create text style: inherit the background from style, but use readable foreground
            let mut text_style = style;
            text_style = text_style
                .fg(self.theme.fg)
                .add_modifier(Modifier::BOLD);

            // Draw label character by character
            for (i, ch) in label.chars().enumerate() {
                let x = label_x + i as u16;
                if x >= inner.x + inner.width {
                    break;
                }
                if label_y < buf.area.bottom() && x < buf.area.right() {
                    let cell = buf.get_mut(x, label_y);
                    cell.set_symbol(&ch.to_string());
                    cell.set_style(text_style);
                }
            }
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
