use crate::tui::{
    state::{
        AppState, PaneId, SourceViewState, SymbolSection,
        SymbolsViewState,
    },
    theme::Theme,
};
use ratatui::{
    prelude::*,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Calculate the floating popup rect for Symbols panel (bottom-right of VM area)
fn symbols_popup_rect(vm_area: Rect) -> Rect {
    // width: 40% of vm_area width
    let width = vm_area.width * 40 / 100;
    // height: minimum 6 lines, maximum half of vm_area height
    let min_h = 6;
    let max_h = vm_area.height / 2;
    let height = std::cmp::max(min_h, max_h);

    Rect {
        x: vm_area.x + vm_area.width.saturating_sub(width),
        y: vm_area.y + vm_area.height.saturating_sub(height),
        width,
        height,
    }
}

pub fn draw(f: &mut Frame, app: &AppState) {
    let theme = &app.theme;
    let size = f.size();

    // Clear and paint the full background to avoid artifacts after resizing.
    f.render_widget(Clear, size);
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), size);

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

    // Render header
    render_header(f, theme, header_area, app);

    // Split main area into Source (left) and VM (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // left: Source
            Constraint::Percentage(40), // right: VM
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
        &app.vm.lines,
        app.vm.scroll_y,
    );

    // Render Symbols popup if visible
    if app.show_symbols_popup {
        let popup_area = symbols_popup_rect(vm_area);
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

/// Render header status bar
fn render_header(f: &mut Frame, theme: &Theme, area: Rect, app: &AppState) {
    let mode = "NORMAL";
    let focus_name = match app.focus {
        PaneId::Source => "Source",
        PaneId::VmCanvas => "VM",
        PaneId::Symbols => "Symbols",
        PaneId::Detail => "Detail",
    };

    let filename = app
        .source
        .filename
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("(no file)");

    let line = app
        .source
        .current_line
        .map(|l| format!(":{}", l))
        .unwrap_or_default();

    let arch = app.debugger.arch.as_deref().unwrap_or("unknown");
    let sym_mode = match app.symbol_index_mode {
        crate::symbols::SymbolIndexMode::None => "none",
        crate::symbols::SymbolIndexMode::DebugOnly => "debug-only",
        crate::symbols::SymbolIndexMode::DebugAndNonDebug => "all",
    };

    let status_text = format!(
        "gdb-memviz TUI  [{}]  focus={}  {}{}  {}  sym={}",
        mode, focus_name, filename, line, arch, sym_mode
    );

    let header = Paragraph::new(status_text).style(
        Style::default()
            .fg(theme.status_fg)
            .bg(theme.status_bg)
            .add_modifier(Modifier::BOLD),
    );

    f.render_widget(header, area);
}

/// Render command line
fn render_cmdline(f: &mut Frame, theme: &Theme, area: Rect) {
    let cmd = Paragraph::new(":").style(Style::default().fg(theme.panel_text).bg(theme.bg));
    f.render_widget(cmd, area);
}

/// Render VM panel
fn render_vm_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    lines: &[String],
    scroll_y: u16,
) {
    // Clear the panel area first to avoid stale characters after resize.
    f.render_widget(Clear, area);

    let text = lines.join("\n");
    let block = bordered_block(" VM Layout ", theme, focused);
    let para = Paragraph::new(text)
        .style(Style::default().fg(theme.panel_text).bg(theme.bg))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
    f.render_widget(para, area);
}

/// Helper to create a bordered block with focus styling
fn bordered_block<'a>(title: &'a str, _theme: &Theme, focused: bool) -> Block<'a> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(style)
}

fn render_source_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    source: &SourceViewState,
) {
    // Clear the panel area first
    f.render_widget(Clear, area);

    let height = area.height.saturating_sub(2); // Subtract borders
    let start = source.scroll_top as usize;
    let end = (start as u32 + height as u32) as usize;

    let mut lines: Vec<Line> = Vec::new();

    for (i, text) in source.lines.iter().enumerate().take(end).skip(start) {
        let line_no = (i + 1) as u32;
        let is_pc = source.current_line == Some(line_no);

        let prefix = if is_pc { "▶ " } else { "  " };
        let content = format!("{:4} {}{}", line_no, prefix, text);

        let mut line = Line::from(content);
        if is_pc {
            line = line.style(Style::default().add_modifier(Modifier::BOLD));
        }

        lines.push(line);
    }

    let title = if let Some(path) = &source.filename {
        format!(" Source: {} ", path.display())
    } else {
        " Source ".to_string()
    };

    let block = bordered_block(&title, theme, focused);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.panel_text).bg(theme.bg))
        .block(block);

    f.render_widget(paragraph, area);
}

fn render_symbols_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    focused: bool,
    symbols: &SymbolsViewState,
) {
    // Clear the panel area first
    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();

    // Locals section
    lines.push(Line::from("locals:"));

    if symbols.locals.is_empty() {
        lines.push(Line::from("  (no locals)"));
    } else {
        for (idx, entry) in symbols.locals.iter().enumerate() {
            let is_selected = matches!(symbols.selected_section, SymbolSection::Locals)
                && symbols.selected_index == idx;

            let content = format!("  {}: {}", idx, entry.value_preview);
            let mut line = Line::from(content);

            if is_selected {
                line = line.style(Style::default().add_modifier(Modifier::REVERSED));
            }

            lines.push(line);
        }
    }

    lines.push(Line::from("")); // Empty line separator

    // Globals section
    lines.push(Line::from("globals:"));

    if symbols.globals.is_empty() {
        lines.push(Line::from("  (no globals)"));
    } else {
        for (idx, entry) in symbols.globals.iter().enumerate() {
            let is_selected = matches!(symbols.selected_section, SymbolSection::Globals)
                && symbols.selected_index == idx;

            let content = format!("  {}: {}", idx, entry.value_preview);
            let mut line = Line::from(content);

            if is_selected {
                line = line.style(Style::default().add_modifier(Modifier::REVERSED));
            }

            lines.push(line);
        }
    }

    let block = bordered_block(" Symbols ", theme, focused);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.panel_text).bg(theme.bg))
        .block(block);

    f.render_widget(paragraph, area);
}
