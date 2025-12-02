use crate::tui::{state::{AppState, PaneId}, theme};
use ratatui::{prelude::*, widgets::{Block, BorderType, Borders, Clear, Paragraph}};

pub mod helpers;
pub mod panels;
pub mod widgets;

use helpers::{inset, symbols_popup_rect};
use panels::{source::render_source_panel, symbols::render_symbols_panel, vm::render_vm_panel};
use widgets::{cmdline::render_cmdline, header::render_header};

pub fn draw(f: &mut Frame, app: &AppState) {
    let theme = theme::theme();
    let full = f.size();

    // Clear and paint the full background to avoid artifacts after resizing.
    f.render_widget(Clear, full);
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), full);

    // Render outer app frame (floating card effect for entire app)
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_dim))
        .style(Style::default().bg(theme.bg));
    f.render_widget(outer_block, full);

    // Content area is inset by 1 on all sides
    let size = inset(full, 5, 1);

    // 3-tier layout: header / main / command line
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // main area
            Constraint::Length(1), // command line
        ])
        .split(size);

    let header_area = layout[0];
    let main_area = layout[1];
    let cmd_area = layout[2];

    // Render header with separator
    render_header(f, theme, header_area, app);

    // Add separator line below header
    let sep_y = header_area.y + header_area.height;
    if sep_y < main_area.y {
        let sep_area = Rect {
            x: header_area.x,
            y: sep_y,
            width: header_area.width,
            height: 1,
        };
        let sep = Paragraph::new(" ".repeat(sep_area.width as usize))
            .style(Style::default().bg(theme.separator));
        f.render_widget(sep, sep_area);
    }

    // Split main area into Source (left) and VM (right) using adjustable ratio
    let left_pct = app.main_split.min(90); // safety clamp
    let right_pct = 100 - left_pct;

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(left_pct),  // left: Source
            Constraint::Percentage(right_pct), // right: VM
        ])
        .split(main_area);

    let source_area = main_chunks[0];
    let vm_area = main_chunks[1];

    // Render Source and VM panels
    render_source_panel(
        f,
        theme,
        source_area,
        app.focus == PaneId::Source,
        &app.source,
    );

    render_vm_panel(
        f,
        theme,
        vm_area,
        app.focus == PaneId::VmCanvas,
        &[],
        0,
        &app.vm.layout,
        app.vm.cursor_addr,
    );

    // Render Symbols popup if visible
    if app.show_symbols_popup {
        let popup_area = symbols_popup_rect(source_area, vm_area, app.symbols_popup_width);
        f.render_widget(Clear, popup_area);
        render_symbols_panel(
            f,
            theme,
            popup_area,
            app.focus == PaneId::Symbols,
            &app.symbols,
        );
    }

    // Render command line
    render_cmdline(f, theme, cmd_area);
}
