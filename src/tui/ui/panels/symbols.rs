use crate::tui::{
    highlight::{highlight_c_line, CCommentState},
    state::{SymbolSection, SymbolsViewState},
    theme::{self, Theme},
};
use ratatui::{prelude::*, text::{Line, Span}, widgets::{Clear, Paragraph}};

use super::super::helpers::{format_symbol_as_c, truncate_with_ellipsis};

pub fn render_symbols_panel(
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

            // Format as C-style statement
            let c_code = format_symbol_as_c(&entry.value_preview);

            // Add indentation
            let indented = format!("  {}", c_code);

            // Truncate if too long (reserve space for padding)
            let max_text_width = inner_width.saturating_sub(2);
            let truncated = truncate_with_ellipsis(&indented, max_text_width);

            // Apply C syntax highlighting
            let mut comment_state = CCommentState::default();
            let highlighted = highlight_c_line(&truncated, &mut comment_state, theme);

            // Convert spans to owned versions (to avoid lifetime issues)
            let mut spans: Vec<Span> = highlighted
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();

            // Calculate padding needed
            let content_len: usize = spans.iter().map(|s| s.content.len()).sum();
            let padding_len = inner_width.saturating_sub(content_len);

            // Add padding to fill the width
            if padding_len > 0 {
                let last_style = spans.last().map(|s| s.style).unwrap_or_default();
                spans.push(Span::styled(" ".repeat(padding_len), last_style));
            }

            // Apply selection background if needed
            if is_selected {
                // Override background for all spans
                for span in &mut spans {
                    span.style = span.style.bg(theme.accent_soft);
                }
            }

            lines.push(Line::from(spans));
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

            // Format as C-style statement
            let c_code = format_symbol_as_c(&entry.value_preview);

            // Add indentation
            let indented = format!("  {}", c_code);

            // Truncate if too long (reserve space for padding)
            let max_text_width = inner_width.saturating_sub(2);
            let truncated = truncate_with_ellipsis(&indented, max_text_width);

            // Apply C syntax highlighting
            let mut comment_state = CCommentState::default();
            let highlighted = highlight_c_line(&truncated, &mut comment_state, theme);

            // Convert spans to owned versions (to avoid lifetime issues)
            let mut spans: Vec<Span> = highlighted
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();

            // Calculate padding needed
            let content_len: usize = spans.iter().map(|s| s.content.len()).sum();
            let padding_len = inner_width.saturating_sub(content_len);

            // Add padding to fill the width
            if padding_len > 0 {
                let last_style = spans.last().map(|s| s.style).unwrap_or_default();
                spans.push(Span::styled(" ".repeat(padding_len), last_style));
            }

            // Apply selection background if needed
            if is_selected {
                // Override background for all spans
                for span in &mut spans {
                    span.style = span.style.bg(theme.accent_soft);
                }
            }

            lines.push(Line::from(spans));
        }
    }

    let block = theme::symbols_popup_block(focused, theme);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.fg).bg(theme.popup_bg))
        .block(block);

    f.render_widget(paragraph, area);
}
