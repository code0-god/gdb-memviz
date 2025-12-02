use crate::tui::theme::{self, Theme};
use crate::tui::ui::widgets::VmMinimap;
use crate::vm::VmLayout;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// VM Layout 패널: [VM map][Address][Hex][ASCII] 4분할 (Address/Hex/ASCII는 고정 폭, VM map만 가변)
pub fn render_vm_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    _lines: &[String],
    _scroll_y: u16,
    vm_layout: &VmLayout,
    cursor_addr: Option<u64>,
) {
    // 1) 전체 영역 클리어 후 바깥 패널 렌더
    f.render_widget(Clear, area);
    let mut block = theme::panel_block(" VM Layout ", focused, theme);
    let border_color = if focused {
        theme.accent
    } else {
        theme.vm_panel_border
    };
    block = block.border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 40 || inner.height < 3 {
        return;
    }

    // 2) 가로 4분할 (VM map 가변, 나머지 고정 폭)
    let bytes_per_line: u16 = 16;
    let addr_width: u16 = 18;
    let hex_width: u16 = bytes_per_line * 3 + 1; // 48
    let ascii_width: u16 = bytes_per_line + 2; // 18
    let vm_min_width: u16 = 16;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(vm_min_width),   // VM map (가변)
            Constraint::Length(addr_width),  // Address (고정)
            Constraint::Length(hex_width),   // Hex (고정)
            Constraint::Length(ascii_width), // ASCII (고정)
        ])
        .split(inner);

    let vm_map_area = chunks[0];
    let addr_area = chunks[1];
    let hex_area = chunks[2];
    let ascii_area = chunks[3];

    if vm_map_area.width == 0 {
        return;
    }

    // 3) Address/Hex/ASCII 패널 Block 생성 후 렌더
    let inner_border = if focused {
        theme.detail_panel_border
    } else {
        theme.border
    };

    let addr_block = Block::default()
        .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM | Borders::RIGHT)
        .title("Address")
        .style(Style::default().bg(theme.vm_panel_bg))
        .border_style(Style::default().fg(inner_border));
    let hex_block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT)
        .title("Hex")
        .style(Style::default().bg(theme.vm_panel_bg))
        .border_style(Style::default().fg(inner_border));
    let ascii_block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT)
        .title("ASCII")
        .style(Style::default().bg(theme.vm_panel_bg))
        .border_style(Style::default().fg(inner_border));

    let addr_inner = addr_block.inner(addr_area);
    let hex_inner = hex_block.inner(hex_area);
    let ascii_inner = ascii_block.inner(ascii_area);

    f.render_widget(addr_block, addr_area);
    f.render_widget(hex_block, hex_area);
    f.render_widget(ascii_block, ascii_area);

    // 4) 더미 데이터 렌더
    let dummy_addr_lines: Vec<Line> = (0..addr_inner.height)
        .map(|i| {
            Line::from(Span::raw(format!(
                "{:016x}",
                0x0000aaaa0000u64 + i as u64 * bytes_per_line as u64
            )))
        })
        .collect();
    let addr_para = Paragraph::new(dummy_addr_lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg));
    f.render_widget(addr_para, addr_inner);

    let dummy_hex_line = "00 11 22 33 44 55 66 77 88 99 aa bb cc dd ee ff";
    let dummy_hex_lines: Vec<Line> = (0..hex_inner.height)
        .map(|_| Line::from(Span::raw(dummy_hex_line)))
        .collect();
    let hex_para = Paragraph::new(dummy_hex_lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg));
    f.render_widget(hex_para, hex_inner);

    let dummy_ascii_line = "................";
    let dummy_ascii_lines: Vec<Line> = (0..ascii_inner.height)
        .map(|_| Line::from(Span::raw(dummy_ascii_line)))
        .collect();
    let ascii_para = Paragraph::new(dummy_ascii_lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg));
    f.render_widget(ascii_para, ascii_inner);

    // 5) minimap 렌더 (VM map 영역)
    let minimap_border = if focused {
        theme.border
    } else {
        theme.border
    };
    let minimap = VmMinimap::new(vm_layout, cursor_addr, theme, minimap_border);
    f.render_widget(minimap, vm_map_area);
}
