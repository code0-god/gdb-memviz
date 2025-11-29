use crate::tui::{
    state::{AppState, Focus, PaneId, PaneNode, SourceViewState, SplitDir},
    theme::Theme,
};
use ratatui::{
    prelude::*,
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};
use std::collections::HashMap;

const STATUS_TEXT: &str = " gdb-memviz TUI (T0.1 skeleton) · sample.c (placeholder) · q: quit ";

pub fn collect_pane_rects(node: &PaneNode, area: Rect, out: &mut HashMap<PaneId, Rect>) {
    match node {
        PaneNode::Leaf(id) => {
            out.insert(*id, area);
        }
        PaneNode::Split {
            dir,
            ratio,
            first,
            second,
        } => {
            let (first_area, second_area) = match dir {
                SplitDir::Vertical => {
                    let w1 = area.width * (*ratio as u16) / 100;
                    let w2 = area.width - w1;
                    (
                        Rect { width: w1, ..area },
                        Rect {
                            x: area.x + w1,
                            width: w2,
                            ..area
                        },
                    )
                }
                SplitDir::Horizontal => {
                    let h1 = area.height * (*ratio as u16) / 100;
                    let h2 = area.height - h1;
                    (
                        Rect { height: h1, ..area },
                        Rect {
                            y: area.y + h1,
                            height: h2,
                            ..area
                        },
                    )
                }
            };
            collect_pane_rects(first, first_area, out);
            collect_pane_rects(second, second_area, out);
        }
    }
}

pub fn draw(f: &mut Frame, app: &AppState) {
    let theme = &app.theme;
    let size = f.size();

    // Clear and paint the full background to avoid artifacts after resizing.
    f.render_widget(Clear, size);
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), size);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(size);

    let status_area = layout[0];
    let main_area = layout[1];

    // Compute pane rectangles from the pane tree
    let mut pane_map = HashMap::new();
    collect_pane_rects(&app.layout.root, main_area, &mut pane_map);

    let source_area = pane_map[&PaneId::Source];
    let vm_area = pane_map[&PaneId::VmCanvas];
    let symbols_area = pane_map[&PaneId::Symbols];
    let detail_area = pane_map[&PaneId::Detail];

    let status_block = theme.status_block();
    let status = Paragraph::new(STATUS_TEXT)
        .style(
            Style::default()
                .fg(theme.status_fg)
                .bg(theme.status_bg)
                .add_modifier(Modifier::BOLD),
        )
        .block(status_block);
    f.render_widget(status, status_area);

    render_source_panel(
        f,
        theme,
        source_area,
        PaneId::Source,
        app.focus,
        &app.source,
    );

    render_panel(
        f,
        theme,
        vm_area,
        " VM Layout ",
        PaneId::VmCanvas,
        app.focus,
        &app.vm.lines,
        app.vm.scroll_y,
    );

    render_panel(
        f,
        theme,
        symbols_area,
        " Symbols (placeholder) ",
        PaneId::Symbols,
        app.focus,
        &app.symbols.lines,
        app.symbols.scroll_y,
    );

    render_panel(
        f,
        theme,
        detail_area,
        " Detail (placeholder) ",
        PaneId::Detail,
        app.focus,
        &app.detail.lines,
        app.detail.scroll_y,
    );
}

fn render_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    panel: Focus,
    focus: Focus,
    lines: &[String],
    scroll_y: u16,
) {
    // Clear the panel area first to avoid stale characters after resize.
    f.render_widget(Clear, area);

    let text = lines.join("\n");
    let block = theme.panel_block_focus(title, panel == focus);
    let para = Paragraph::new(text)
        .style(Style::default().fg(theme.panel_text).bg(theme.bg))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
    f.render_widget(para, area);
}

fn render_source_panel(
    f: &mut Frame,
    theme: &Theme,
    area: Rect,
    panel: Focus,
    focus: Focus,
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

    let block = theme.panel_block_focus(&title, panel == focus);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.panel_text).bg(theme.bg))
        .block(block);

    f.render_widget(paragraph, area);
}
