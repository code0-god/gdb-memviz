use ratatui::{prelude::*, text::{Line, Span}};

/// Inset a rect by dx/dy on all sides
pub fn inset(rect: Rect, dx: u16, dy: u16) -> Rect {
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
pub fn symbols_popup_rect(source_area: Rect, _vm_area: Rect, width_cols: u16) -> Rect {
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

/// Format a symbol entry as a C-style statement
/// Example: "int x = 42" -> "int x = 42;"
pub fn format_symbol_as_c(value_preview: &str) -> String {
    // value_preview is already in format: "type name = value"
    // We just need to add semicolon
    format!("{};", value_preview)
}

/// Truncate a string with ellipsis if it exceeds max_width
/// Returns the truncated string (without padding)
pub fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    if text.len() <= max_width {
        text.to_string()
    } else {
        // Reserve 4 chars for " ..."
        let available = max_width.saturating_sub(4);
        if available == 0 {
            "...".to_string()
        } else {
            let truncated: String = text.chars().take(available).collect();
            format!("{} ...", truncated)
        }
    }
}

/// Pad or truncate a line to the specified width
pub fn pad_or_truncate_line(mut line: Line, width: usize) -> Line {
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
