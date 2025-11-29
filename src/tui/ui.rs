use crate::tui::{
    state::{AppState, PaneId, SourceViewState, SymbolSection, SymbolsViewState},
    theme::{self, Theme},
};
use ratatui::{
    prelude::*,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

/// Inset a rect by dx/dy on all sides
fn inset(rect: Rect, dx: u16, dy: u16) -> Rect {
    let x = rect.x.saturating_add(dx);
    let y = rect.y.saturating_add(dy);
    let width = rect.width.saturating_sub(dx * 2);
    let height = rect.height.saturating_sub(dy * 2);
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Calculate the floating popup rect for Symbols panel (top-right of Source area)
fn symbols_popup_rect(source_area: Rect, _vm_area: Rect, width_cols: u16) -> Rect {
    // width: absolute column count, clamped to source_area width
    let width = width_cols.min(source_area.width.saturating_sub(2));
    // height: minimum 6 lines, maximum 40% of source_area height
    let min_h = 6;
    let max_h = source_area.height * 40 / 100;
    let height = std::cmp::max(min_h, max_h);

    // x: Source's right edge minus width (right-aligned within Source)
    let x = source_area.x + source_area.width.saturating_sub(width);
    // y: Source's top edge (top-aligned)
    let y = source_area.y;

    Rect {
        x,
        y,
        width,
        height,
    }
}

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
        &app.vm.lines,
        app.vm.scroll_y,
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

    // Add separator line above command line
    if cmd_area.y > 0 {
        let sep_area = Rect {
            x: cmd_area.x,
            y: cmd_area.y.saturating_sub(1),
            width: cmd_area.width,
            height: 1,
        };
        let sep = Paragraph::new(" ".repeat(sep_area.width as usize))
            .style(Style::default().bg(theme.separator));
        f.render_widget(sep, sep_area);
    }

    // Render command line
    render_cmdline(f, theme, cmd_area);
}

/// Render header status bar with styled segments (oatmeal-style: left info, right hints)
fn render_header(f: &mut Frame, theme: &Theme, area: Rect, app: &AppState) {
    // Split header into left (info) and right (key hints)
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Left: app badge + mode + status info
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

    let line_str = app
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

    let left_spans = vec![
        Span::styled(
            " gdb-memviz ",
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("[{}]", mode),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("focus:{}  ", focus_name),
            Style::default().fg(theme.fg_dim),
        ),
        Span::styled(
            format!("{}{}  ", filename, line_str),
            Style::default().fg(theme.status_fg),
        ),
        Span::styled(format!("{}  ", arch), Style::default().fg(theme.fg_dim)),
        Span::styled(
            format!("sym={}", sym_mode),
            Style::default().fg(theme.fg_dim),
        ),
    ];

    let left = Paragraph::new(Line::from(left_spans))
        .style(Style::default().bg(theme.status_bg).fg(theme.status_fg));
    f.render_widget(left, header_chunks[0]);

    // Right: key hints
    let right_text = Line::from(vec![
        Span::styled("Ctrl+h/l", Style::default().fg(theme.fg_dim)),
        Span::raw(" focus  "),
        Span::styled("Ctrl+s", Style::default().fg(theme.fg_dim)),
        Span::raw(" symbols  "),
        Span::styled("F5", Style::default().fg(theme.fg_dim)),
        Span::raw(" next  "),
        Span::styled("q", Style::default().fg(theme.fg_dim)),
        Span::raw(" quit"),
    ]);

    let right = Paragraph::new(right_text)
        .alignment(Alignment::Right)
        .style(Style::default().bg(theme.status_bg).fg(theme.fg_dim));
    f.render_widget(right, header_chunks[1]);
}

/// Render command line in Neovim-style
fn render_cmdline(f: &mut Frame, theme: &Theme, area: Rect) {
    let line = Line::from(vec![
        Span::styled(":", Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled("(future command mode)", Style::default().fg(theme.fg_dim)),
    ]);

    let cmd =
        Paragraph::new(line).style(Style::default().bg(theme.cmdline_bg).fg(theme.cmdline_fg));
    f.render_widget(cmd, area);
}

/// Render VM panel with colored region labels
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

    // Calculate available width inside the panel (subtract borders)
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();

    for (i, text) in source.lines.iter().enumerate().take(end).skip(start) {
        let line_no = (i + 1) as u32;
        let is_pc = source.current_line == Some(line_no);

        let prefix = if is_pc { "▶ " } else { "  " };

        // Build the line content: line_num + prefix + code
        let mut line_content = format!("{:4} {}{}", line_no, prefix, text);

        // Pad to full width for PC line highlight to span entire panel
        if line_content.len() < inner_width {
            line_content.push_str(&" ".repeat(inner_width - line_content.len()));
        }

        // Apply styling
        if is_pc {
            // PC line: full width highlight with accent_soft background
            let line = Line::from(Span::styled(
                line_content,
                Style::default()
                    .bg(theme.accent_soft)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(line);
        } else {
            // Normal line: line number in dim, rest in normal color
            let line_num_span =
                Span::styled(format!("{:4} ", line_no), Style::default().fg(theme.fg_dim));
            let rest = format!("{}{}", prefix, text);
            let rest_padded = if rest.len() < inner_width - 5 {
                format!("{}{}", rest, " ".repeat(inner_width - 5 - rest.len()))
            } else {
                rest
            };
            let rest_span = Span::styled(rest_padded, Style::default().fg(theme.fg));

            lines.push(Line::from(vec![line_num_span, rest_span]));
        }
    }

    let title = if let Some(path) = &source.filename {
        format!("Source: {}", path.display())
    } else {
        "Source".to_string()
    };

    let block = theme::panel_block(&title, focused, theme);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg))
        .block(block)
        .wrap(Wrap { trim: false });

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

    // Calculate available width inside the panel (subtract borders)
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();

    // Locals section header
    let mut header_text = "locals:".to_string();
    if header_text.len() < inner_width {
        header_text.push_str(&" ".repeat(inner_width - header_text.len()));
    }
    lines.push(Line::from(Span::styled(
        header_text,
        Style::default()
            .fg(theme.fg_dim)
            .add_modifier(Modifier::BOLD),
    )));

    if symbols.locals.is_empty() {
        let mut text = "  (no locals)".to_string();
        if text.len() < inner_width {
            text.push_str(&" ".repeat(inner_width - text.len()));
        }
        lines.push(Line::from(Span::styled(
            text,
            Style::default().fg(theme.fg_dim),
        )));
    } else {
        for (idx, entry) in symbols.locals.iter().enumerate() {
            let is_selected = matches!(symbols.selected_section, SymbolSection::Locals)
                && symbols.selected_index == idx;

            let mut content = format!("  {}: {}", idx, entry.value_preview);

            // Pad to full width for full-width highlight
            if content.len() < inner_width {
                content.push_str(&" ".repeat(inner_width - content.len()));
            }

            let style = if is_selected {
                Style::default()
                    .bg(theme.accent_soft)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            lines.push(Line::from(Span::styled(content, style)));
        }
    }

    // Empty line separator
    let mut sep_text = String::new();
    if sep_text.len() < inner_width {
        sep_text.push_str(&" ".repeat(inner_width - sep_text.len()));
    }
    lines.push(Line::from(sep_text));

    // Globals section header
    let mut globals_header = "globals:".to_string();
    if globals_header.len() < inner_width {
        globals_header.push_str(&" ".repeat(inner_width - globals_header.len()));
    }
    lines.push(Line::from(Span::styled(
        globals_header,
        Style::default()
            .fg(theme.fg_dim)
            .add_modifier(Modifier::BOLD),
    )));

    if symbols.globals.is_empty() {
        let mut text = "  (no globals)".to_string();
        if text.len() < inner_width {
            text.push_str(&" ".repeat(inner_width - text.len()));
        }
        lines.push(Line::from(Span::styled(
            text,
            Style::default().fg(theme.fg_dim),
        )));
    } else {
        for (idx, entry) in symbols.globals.iter().enumerate() {
            let is_selected = matches!(symbols.selected_section, SymbolSection::Globals)
                && symbols.selected_index == idx;

            let mut content = format!("  {}: {}", idx, entry.value_preview);

            // Pad to full width for full-width highlight
            if content.len() < inner_width {
                content.push_str(&" ".repeat(inner_width - content.len()));
            }

            let style = if is_selected {
                Style::default()
                    .bg(theme.accent_soft)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            lines.push(Line::from(Span::styled(content, style)));
        }
    }

    let block = theme::symbols_popup_block(focused, theme);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.fg).bg(theme.panel_bg))
        .block(block);

    f.render_widget(paragraph, area);
}
