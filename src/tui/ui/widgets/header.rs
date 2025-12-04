use crate::tui::{keymap::KeyMap, state::{AppState, PaneId}, theme::Theme};
use ratatui::{prelude::*, text::{Line, Span}, widgets::Paragraph};

/// Render header status bar with styled segments (oatmeal-style: left info, right hints)
pub fn render_header(f: &mut Frame, theme: &Theme, area: Rect, app: &AppState) {
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

    // Right: key hints - dynamically generated from keymap
    let keymap = KeyMap::new();
    let hints = keymap.get_status_hints();

    let mut right_spans = Vec::new();
    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            right_spans.push(Span::raw("  "));
        }
        right_spans.push(Span::styled(key, Style::default().fg(theme.fg_dim)));
        right_spans.push(Span::raw(" : "));
        right_spans.push(Span::raw(desc));
    }

    let right_text = Line::from(right_spans);

    let right = Paragraph::new(right_text)
        .alignment(Alignment::Right)
        .style(Style::default().bg(theme.status_bg).fg(theme.fg_dim));
    f.render_widget(right, header_chunks[1]);
}
