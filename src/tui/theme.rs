use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,
    #[allow(dead_code)]
    pub fg: Color,

    #[allow(dead_code)]
    pub status_fg: Color,
    pub status_bg: Color,

    pub panel_border: Color,
    pub panel_title: Color,
    pub panel_text: Color,

    #[allow(dead_code)]
    pub locals_normal: Style,
    #[allow(dead_code)]
    pub locals_selected: Style,

    #[allow(dead_code)]
    pub vm_stack: Style,
    #[allow(dead_code)]
    pub vm_heap: Style,
    #[allow(dead_code)]
    pub vm_data: Style,
    #[allow(dead_code)]
    pub vm_text: Style,
    #[allow(dead_code)]
    pub vm_highlight: Style,
}

impl Theme {
    pub fn default() -> Self {
        let fg = Color::White;
        let bg = Color::Black;
        let panel_text = fg;
        let locals_normal = Style::default().fg(panel_text).bg(bg);
        let locals_selected = locals_normal.add_modifier(Modifier::REVERSED);

        Self {
            bg,
            fg,

            status_fg: fg,
            status_bg: bg,

            panel_border: Color::Gray,
            panel_title: Color::White,
            panel_text,

            locals_normal,
            locals_selected,

            vm_stack: Style::default().fg(Color::LightBlue),
            vm_heap: Style::default().fg(Color::LightGreen),
            vm_data: Style::default().fg(Color::Yellow),
            vm_text: Style::default().fg(Color::Magenta),
            vm_highlight: Style::default().fg(Color::Cyan),
        }
    }

    pub fn panel_block<'a>(&self, title: &'a str) -> Block<'a> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(self.panel_border))
            .style(Style::default().bg(self.bg))
            .title(title)
            .title_style(Style::default().fg(self.panel_title))
    }

    pub fn panel_block_focus<'a>(&self, title: &'a str, focused: bool) -> Block<'a> {
        let base = self.panel_block(title);
        if !focused {
            return base;
        }
        let highlight_color = self.vm_highlight.fg.unwrap_or(self.panel_border);
        base.border_style(Style::default().fg(highlight_color))
    }

    pub fn status_block(&self) -> Block<'_> {
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(self.panel_border))
    }
}
