use crate::tui::{
    highlight::{highlight_c_line, CCommentState},
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

    // Render panel block with simple "Source" title
    let block = theme::panel_block("Source", focused, theme);
    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    // Early exit if not enough space
    if inner.height < 2 {
        return;
    }

    // Split inner area into file statusline (1 line) and code area (rest)
    let file_bar_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let code_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height - 1,
    };

    // Render file statusline (basename + line number)
    let file_label = if let Some(path) = &source.filename {
        let basename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");

        if let Some(line) = source.current_line {
            format!("{}:{}", basename, line)
        } else {
            basename.to_string()
        }
    } else {
        "(no file)".to_string()
    };

    // Statusline color is independent from accent_soft.
    let file_bar_style = Style::default()
        .bg(theme.file_status_bg)
        .fg(theme.file_status_fg);

    let file_bar_line = Line::from(Span::styled(file_label, file_bar_style));
    let file_bar = Paragraph::new(file_bar_line).alignment(Alignment::Left);
    f.render_widget(file_bar, file_bar_area);

    // Render code with syntax highlighting
    let visible_height = code_area.height as usize;

    // Initialize comment state for tracking multi-line block comments
    let mut comment_state = CCommentState::default();

    // We need to process all lines from the beginning to maintain correct comment state,
    // but we only render the visible ones
    for line_index in 0..source.lines.len() {
        let line_text = &source.lines[line_index];

        // Update the comment state by processing this line
        let highlighted = highlight_c_line(line_text, &mut comment_state, theme);

        // Only render if this line is in the visible range
        let row = line_index.saturating_sub(source.scroll_top as usize);
        if row >= visible_height {
            continue; // Past visible area
        }
        if line_index < source.scroll_top as usize {
            continue; // Before visible area
        }

        let y = code_area.y + row as u16;
        let line_no = line_index + 1;

        // Build spans (marker + gutter + code)
        let (pc_marker, marker_color) = if source.current_line == Some(line_no as u32) {
            ("▶", theme.pc_marker)
        } else {
            (" ", theme.fg_dim)
        };
        let marker_span = Span::styled(
            pc_marker,
            Style::default().fg(marker_color).bg(theme.panel_bg),
        );
        let gutter = format!("{:>4} ", line_no); // 5 columns
        let gutter_span = Span::styled(gutter, Style::default().fg(theme.fg_dim));

        // Render marker column separately
        let marker_width: u16 = 1;
        let spacer_width: u16 = 2; // gap after marker before gutter
        let marker_para = Paragraph::new(Line::from(vec![marker_span]))
            .style(Style::default().bg(theme.panel_bg));
        f.render_widget(
            marker_para,
            Rect {
                x: code_area.x,
                y,
                width: marker_width,
                height: 1,
            },
        );

        // Gutter + code
        let mut spans: Vec<Span> = Vec::new();
        spans.push(gutter_span);
        spans.extend(highlighted.spans.into_iter());

        let mut line = Line::from(spans);

        let is_pc_line = if let Some(pc_line) = source.current_line {
            pc_line as usize == line_index + 1
        } else {
            false
        };

        // Pad or truncate the line to remaining width
        let remaining_width =
            code_area.width.saturating_sub(marker_width + spacer_width) as usize;
        line = pad_or_truncate_line(line, remaining_width);

        // Apply background to the gutter+code segment
        let mut para_style = Style::default().bg(theme.panel_bg);
        if is_pc_line {
            // Only override background to keep syntax highlight foreground intact.
            para_style = para_style.bg(theme.accent_soft);
        }

        let paragraph = Paragraph::new(line).style(para_style);
        f.render_widget(
            paragraph,
            Rect {
                x: code_area.x + marker_width + spacer_width,
                y,
                width: code_area
                    .width
                    .saturating_sub(marker_width + spacer_width),
                height: 1,
            },
        );
    }

    // Render empty lines if there are fewer source lines than visible height
    for row in source
        .lines
        .len()
        .saturating_sub(source.scroll_top as usize)..visible_height
    {
        let y = code_area.y + row as u16;
        let marker_width: u16 = 1;
        // marker column
        let marker_para = Paragraph::new(Line::from(vec![Span::styled(
            " ",
            Style::default().bg(theme.panel_bg),
        )]))
        .style(Style::default().bg(theme.panel_bg));
        f.render_widget(
            marker_para,
            Rect {
                x: code_area.x,
                y,
                width: marker_width,
                height: 1,
            },
        );

        let spacer_width: u16 = 2;
        // spacer + gutter + padding
        let spacer_gutter = "     ".to_string(); // line number space (5 cols)
        let spans = vec![Span::styled(
            spacer_gutter,
            Style::default().fg(theme.fg_dim).bg(theme.panel_bg),
        )];
        let line = pad_or_truncate_line(
            Line::from(spans),
            code_area
                .width
                .saturating_sub(marker_width + spacer_width) as usize,
        );

        let paragraph = Paragraph::new(line).style(Style::default().bg(theme.panel_bg));
        f.render_widget(
            paragraph,
            Rect {
                x: code_area.x + marker_width + spacer_width,
                y,
                width: code_area
                    .width
                    .saturating_sub(marker_width + spacer_width),
                height: 1,
            },
        );
    }
}

/// Pad or truncate a line to the specified width
fn pad_or_truncate_line(mut line: Line, width: usize) -> Line {
    // Calculate current line width
    let current_width: usize = line.spans.iter().map(|s| s.content.len()).sum();

    if current_width < width {
        // Pad with spaces
        let padding = " ".repeat(width - current_width);
        let last_style = line
            .spans
            .last()
            .map(|s| s.style)
            .unwrap_or_else(|| Style::default());
        line.spans.push(Span::styled(padding, last_style));
    } else if current_width > width {
        // Truncate
        let mut accumulated = 0;
        let mut new_spans = Vec::new();
        for span in line.spans {
            let span_len = span.content.len();
            if accumulated + span_len <= width {
                new_spans.push(span);
                accumulated += span_len;
            } else {
                let remaining = width - accumulated;
                if remaining > 0 {
                    let truncated = &span.content[..remaining];
                    new_spans.push(Span::styled(truncated.to_string(), span.style));
                }
                break;
            }
        }
        line.spans = new_spans;
    }

    line
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
