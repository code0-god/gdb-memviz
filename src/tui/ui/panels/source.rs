use crate::tui::{
    highlight::{highlight_c_line, CCommentState},
    state::SourceViewState,
    theme::{self, Theme},
};
use ratatui::{prelude::*, text::{Line, Span}, widgets::{Clear, Paragraph}};

use super::super::helpers::pad_or_truncate_line;

pub fn render_source_panel(
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
        let remaining_width = code_area.width.saturating_sub(marker_width + spacer_width) as usize;
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
                width: code_area.width.saturating_sub(marker_width + spacer_width),
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
            code_area.width.saturating_sub(marker_width + spacer_width) as usize,
        );

        let paragraph = Paragraph::new(line).style(Style::default().bg(theme.panel_bg));
        f.render_widget(
            paragraph,
            Rect {
                x: code_area.x + marker_width + spacer_width,
                y,
                width: code_area.width.saturating_sub(marker_width + spacer_width),
                height: 1,
            },
        );
    }
}
