use crate::tui::state::AppState;
use crate::tui::theme::{self, Theme};
use crate::tui::ui::widgets::VmMinimap;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// VM Layout 패널: [VM map][Address][Hex][ASCII] 4분할 (Address/Hex/ASCII는 고정 폭, VM map만 가변)
pub fn render_vm_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    app: &mut AppState,
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
    let addr_width: u16 = 20;
    let hex_width: u16 = bytes_per_line * 3; // 48
    let ascii_width: u16 = bytes_per_line + 1; // 17
    let vm_min_width: u16 = 16;

    // 총 최소 필요 폭(고정폭 합 + minimap 최소폭) 확보 확인
    let fixed_total = addr_width + hex_width + ascii_width;
    if inner.width < fixed_total + vm_min_width {
        return;
    }

    let vm_width = inner.width.saturating_sub(fixed_total);
    if vm_width < vm_min_width {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(vm_width),    // VM map (가변, 최소 확보)
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

    // 4) lines_per_page 설정
    let lines_per_page = addr_inner.height as u16;
    app.vm.hex.lines_per_page = lines_per_page;

    // top_addr가 0이고 VM 영역이 있다면 첫 region 시작 주소로 초기화
    if app.vm.hex.top_addr == 0 && !app.vm.layout.regions.is_empty() {
        app.vm.hex.top_addr = app.vm.layout.regions[0].start;
        app.vm.hex.cursor_addr = app.vm.hex.top_addr;
    }

    // 필요 시 현재 페이지를 로드
    let need_refresh = app.vm.hex.buf.is_empty()
        || app.vm.hex.last_loaded_top_addr != app.vm.hex.top_addr
        || app.vm.hex.last_loaded_lines_per_page != app.vm.hex.lines_per_page;

    if need_refresh {
        if let Err(err) = app.refresh_vm_hex_page() {
            crate::logger::log_debug(&format!(
                "[vm] refresh_vm_hex_page error: {:?}",
                err
            ));
        }
    }

    // 5) Address/Hex/ASCII 라인 빌드
    let bpl = app.vm.hex.bytes_per_line as usize;
    let cursor_addr = app.vm.hex.cursor_addr;
    let valid_len = app.vm.hex.valid_len;
    let buf = &app.vm.hex.buf;
    let top_addr = app.vm.hex.top_addr;

    let mut addr_lines: Vec<Line> = Vec::new();
    let mut hex_lines: Vec<Line> = Vec::new();
    let mut ascii_lines: Vec<Line> = Vec::new();

    for row in 0..(addr_inner.height as usize) {
        let line_addr = top_addr + (row as u64) * (bpl as u64);

        // 현재 줄에 커서가 있는지 확인
        let cursor_on_this_line = cursor_addr >= line_addr
            && cursor_addr < line_addr + (bpl as u64);

        // Address
        let addr_str = format!("0x{:016x}", line_addr);
        let addr_style = if cursor_on_this_line {
            Style::default()
                .fg(theme.fg)
                .bg(theme.vm_panel_bg)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg).bg(theme.vm_panel_bg)
        };
        addr_lines.push(Line::from(Span::styled(addr_str, addr_style)));

        // Hex/ASCII
        let mut hex_spans: Vec<Span> = Vec::new();
        let mut ascii_spans: Vec<Span> = Vec::new();

        for col in 0..bpl {
            let addr = line_addr + (col as u64);
            let idx = row * bpl + col;

            let byte_opt = if idx < valid_len {
                Some(buf[idx])
            } else {
                None
            };

            let is_cursor = addr == cursor_addr;

            // 한 바이트에 대한 hex/ASCII 문자열
            let (hex_str, ascii_ch) = match byte_opt {
                Some(b) => {
                    let ch = if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    };
                    (format!("{:02x}", b), ch)
                }
                None => ("..".to_string(), '.'),
            };

            // 기본 스타일
            let mut hex_style = Style::default().fg(theme.fg).bg(theme.vm_panel_bg);
            let mut ascii_style = hex_style;

            if is_cursor {
                // hex 두 글자와 ASCII 한 글자만 반전
                hex_style = hex_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                ascii_style = ascii_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }

            // hex 두 글자
            hex_spans.push(Span::styled(hex_str, hex_style));
            // hex 바이트 사이 공백 (하이라이트하지 않음)
            if col + 1 < bpl {
                hex_spans.push(Span::raw(" "));
            }

            // ascii 한 글자
            ascii_spans.push(Span::styled(ascii_ch.to_string(), ascii_style));
        }

        hex_lines.push(Line::from(hex_spans));
        ascii_lines.push(Line::from(ascii_spans));
    }

    // Paragraph로 렌더링
    let addr_para = Paragraph::new(addr_lines)
        .style(Style::default().fg(theme.fg).bg(theme.vm_panel_bg));
    f.render_widget(addr_para, addr_inner);

    let hex_para = Paragraph::new(hex_lines)
        .style(Style::default().fg(theme.fg).bg(theme.vm_panel_bg));
    f.render_widget(hex_para, hex_inner);

    let ascii_para = Paragraph::new(ascii_lines)
        .style(Style::default().fg(theme.fg).bg(theme.vm_panel_bg));
    f.render_widget(ascii_para, ascii_inner);

    // 6) minimap 렌더 (VM map 영역)
    let minimap_cursor = if app.vm.hex.cursor_addr != 0 {
        Some(app.vm.hex.cursor_addr)
    } else {
        None
    };

    let minimap_border = if focused {
        theme.border
    } else {
        theme.border
    };
    let minimap = VmMinimap::new(&app.vm.layout, minimap_cursor, theme, minimap_border);
    f.render_widget(minimap, vm_map_area);

    // 7) Jump mode popup (if active)
    if app.vm.jump.active {
        let mut popup_width: u16 = 40;
        let mut popup_height: u16 = if app.vm.jump.error.is_some() { 4 } else { 3 };

         // hex 패널 크기를 넘지 않도록 클램프
        if popup_width > hex_area.width {
            popup_width = hex_area.width;
        }
        if popup_height > hex_area.height {
            popup_height = hex_area.height;
        }

        // hex 패널(테두리 포함) 전체의 우측 하단에 붙이기
        let popup_x = hex_area.x + hex_area.width.saturating_sub(popup_width);
        let popup_y = hex_area.y + hex_area.height.saturating_sub(popup_height);
        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

        let block = Block::default()
            .title(" Jump to address ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.jump_panel_border))
            .style(Style::default().bg(theme.panel_bg));

        let inner = block.inner(popup_area);

        // 첫 줄: 프롬프트 + 입력
        let prompt = format!("addr: {}", app.vm.jump.input);

        let mut lines = vec![Line::from(Span::raw(prompt))];

        // 둘째 줄에 오류 메시지 표시 (있다면)
        if let Some(err) = &app.vm.jump.error {
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(theme.error),
            )));
        }

        let para = Paragraph::new(lines).style(Style::default().fg(theme.fg).bg(theme.panel_bg));

        // 기존 화면 위에 덮어쓰기
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);
        f.render_widget(para, inner);
    }
}
