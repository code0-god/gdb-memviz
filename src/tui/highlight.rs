use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme::Theme;

const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return",
    "goto", "sizeof",
];

const TYPES: &[&str] = &[
    "void", "char", "short", "int", "long", "float", "double", "signed", "unsigned", "struct",
    "union", "enum", "typedef", "const", "volatile", "static", "extern", "register",
];

/// State for tracking multi-line C comments across lines
#[derive(Debug, Default, Clone, Copy)]
pub struct CCommentState {
    pub in_block_comment: bool,
}

/// Highlight a single C/C++ source line using a simple heuristic highlighter.
/// Returns a Line where each Span has an appropriate foreground color.
/// The state tracks multi-line block comments across lines.
pub fn highlight_c_line<'a>(line: &'a str, state: &mut CCommentState, theme: &Theme) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();

    // If we're already in a block comment from a previous line
    if state.in_block_comment {
        if let Some(end_idx) = line.find("*/") {
            // Block comment ends on this line
            let comment_end = end_idx + 2;
            spans.push(Span::styled(
                &line[..comment_end],
                Style::default().fg(theme.syntax_comment),
            ));
            state.in_block_comment = false;

            // Process the rest of the line after the block comment
            if comment_end < line.len() {
                spans.extend(highlight_line_impl(&line[comment_end..], state, theme));
            }
        } else {
            // Entire line is still in block comment
            spans.push(Span::styled(
                line,
                Style::default().fg(theme.syntax_comment),
            ));
        }
    } else {
        // Not currently in a block comment
        spans.extend(highlight_line_impl(line, state, theme));
    }

    Line::from(spans)
}

/// Internal implementation for highlighting a line not in a block comment
fn highlight_line_impl<'a>(
    line: &'a str,
    state: &mut CCommentState,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let mut spans: Vec<Span<'a>> = Vec::new();

    // Check for line comment first (takes precedence)
    if let Some(line_comment_idx) = line.find("//") {
        // Process code before the line comment
        if line_comment_idx > 0 {
            spans.extend(highlight_code_and_block_comments(
                &line[..line_comment_idx],
                state,
                theme,
            ));
        }

        // Add the line comment
        spans.push(Span::styled(
            &line[line_comment_idx..],
            Style::default().fg(theme.syntax_comment),
        ));
    } else {
        // No line comment, just handle code and block comments
        spans.extend(highlight_code_and_block_comments(line, state, theme));
    }

    spans
}

/// Highlight code while handling block comments
fn highlight_code_and_block_comments<'a>(
    text: &'a str,
    state: &mut CCommentState,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let mut spans: Vec<Span<'a>> = Vec::new();

    // Look for block comment start
    if let Some(block_start_idx) = text.find("/*") {
        // Process code before the block comment
        if block_start_idx > 0 {
            spans.extend(highlight_code_part(&text[..block_start_idx], theme));
        }

        // Check if block comment ends on the same line
        if let Some(relative_end_idx) = text[block_start_idx + 2..].find("*/") {
            let block_end_idx = block_start_idx + 2 + relative_end_idx + 2;

            // Add the block comment
            spans.push(Span::styled(
                &text[block_start_idx..block_end_idx],
                Style::default().fg(theme.syntax_comment),
            ));

            // Process code after the block comment (recursively to handle multiple block comments)
            if block_end_idx < text.len() {
                spans.extend(highlight_code_and_block_comments(
                    &text[block_end_idx..],
                    state,
                    theme,
                ));
            }
        } else {
            // Block comment doesn't close on this line
            spans.push(Span::styled(
                &text[block_start_idx..],
                Style::default().fg(theme.syntax_comment),
            ));
            state.in_block_comment = true;
        }
    } else {
        // No block comment, just normal code
        spans.extend(highlight_code_part(text, theme));
    }

    spans
}

fn highlight_code_part<'a>(code: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut chars = code.char_indices().peekable();
    let mut in_string = false;
    let mut current_start = 0;

    while let Some((i, ch)) = chars.next() {
        if ch == '"' {
            if in_string {
                // End of string: emit from current_start to after this quote
                let end = i + ch.len_utf8();
                spans.push(Span::styled(
                    &code[current_start..end],
                    Style::default().fg(theme.syntax_string),
                ));
                in_string = false;
                current_start = end;
            } else {
                // Start of string: emit any non-string content before this
                if i > current_start {
                    spans.extend(highlight_non_string(&code[current_start..i], theme));
                }
                in_string = true;
                current_start = i;
            }
        }
    }

    // Handle remaining content
    if in_string {
        // Unterminated string: highlight the rest as string
        spans.push(Span::styled(
            &code[current_start..],
            Style::default().fg(theme.syntax_string),
        ));
    } else if current_start < code.len() {
        // Highlight remaining non-string content
        spans.extend(highlight_non_string(&code[current_start..], theme));
    }

    spans
}

fn highlight_non_string<'a>(text: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut current_pos = 0;

    for (start, end, token) in tokenize(text) {
        // Emit any whitespace/punctuation between tokens
        if start > current_pos {
            spans.push(Span::styled(
                &text[current_pos..start],
                Style::default().fg(theme.syntax_identifier),
            ));
        }

        // Determine token color
        let style = if KEYWORDS.contains(&token) {
            Style::default().fg(theme.syntax_keyword)
        } else if TYPES.contains(&token) {
            Style::default().fg(theme.syntax_type)
        } else if is_number(token) {
            Style::default().fg(theme.syntax_number)
        } else {
            Style::default().fg(theme.syntax_identifier)
        };

        spans.push(Span::styled(&text[start..end], style));
        current_pos = end;
    }

    // Emit any remaining text
    if current_pos < text.len() {
        spans.push(Span::styled(
            &text[current_pos..],
            Style::default().fg(theme.syntax_identifier),
        ));
    }

    spans
}

/// Tokenize text into (start, end, token) tuples for alphanumeric/underscore tokens
fn tokenize(text: &str) -> Vec<(usize, usize, &str)> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;

    for (i, ch) in text.char_indices() {
        if ch.is_alphanumeric() || ch == '_' {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start {
            tokens.push((s, i, &text[s..i]));
            start = None;
        }
    }

    // Handle token at end of string
    if let Some(s) = start {
        tokens.push((s, text.len(), &text[s..]));
    }

    tokens
}

fn is_number(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }

    // Handle hex numbers (0x...)
    if token.starts_with("0x") || token.starts_with("0X") {
        return token[2..].chars().all(|c| c.is_ascii_hexdigit());
    }

    // Handle binary numbers (0b...)
    if token.starts_with("0b") || token.starts_with("0B") {
        return token[2..].chars().all(|c| c == '0' || c == '1');
    }

    // Handle decimal numbers (including floats)
    token.chars().all(|c| c.is_ascii_digit() || c == '.')
}
