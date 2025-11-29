use anyhow::Result;
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, Stdout},
    time::Duration,
};

pub mod state;
pub mod theme;
pub mod ui;

use state::{AppState, Focus, PaneNode, SplitDir};

pub fn run_tui() -> Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    enable_raw_mode()?;
    if let Err(e) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
        disable_raw_mode().ok();
        return Err(e.into());
    }
    let keyboard_enhanced = enable_keyboard_enhancement(terminal.backend_mut());

    let mut app = AppState::default();
    let result = event_loop(&mut terminal, &mut app);
    let cleanup_result = restore_terminal(&mut terminal, keyboard_enhanced);

    result.and(cleanup_result)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    keyboard_enhanced: bool,
) -> Result<()> {
    disable_raw_mode()?;
    if keyboard_enhanced {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn enable_keyboard_enhancement(backend: &mut CrosstermBackend<Stdout>) -> bool {
    let debug_keys = std::env::var("MEMVIZ_TUI_DEBUG_KEYS").is_ok();
    match supports_keyboard_enhancement() {
        Ok(true) => {
            let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
            if execute!(backend, PushKeyboardEnhancementFlags(flags)).is_ok() {
                return true;
            }
            if debug_keys {
                eprintln!("[tui-keyboard] failed to push enhancement flags");
            }
        }
        Ok(false) => {
            if debug_keys {
                eprintln!("[tui-keyboard] keyboard enhancement not supported");
            }
        }
        Err(err) => {
            if debug_keys {
                eprintln!("[tui-keyboard] failed to query keyboard support: {err}");
            }
        }
    }
    false
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut AppState) -> Result<()> {
    let debug_keys = std::env::var("MEMVIZ_TUI_DEBUG_KEYS").is_ok();
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if debug_keys {
                eprintln!("[tui-ev] {:?}", ev);
            }
            if let Event::Key(key_event) = ev {
                if handle_key(key_event, app) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut AppState) -> bool {
    let press_or_repeat = matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat);

    // 1) Exit keys
    if press_or_repeat
        && (matches!(key.code, KeyCode::Char('q'))
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)))
    {
        return true;
    }

    // 2) Ctrl + h/j/k/l for focus movement
    if press_or_repeat && key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('h') => move_focus_dir(app, Direction::Left),
            KeyCode::Char('j') => move_focus_dir(app, Direction::Down),
            KeyCode::Char('k') => move_focus_dir(app, Direction::Up),
            KeyCode::Char('l') => move_focus_dir(app, Direction::Right),

            // Resize: Ctrl + arrows
            KeyCode::Left => resize_current(app, SplitDir::Vertical, -5),
            KeyCode::Right => resize_current(app, SplitDir::Vertical, 5),
            KeyCode::Up => resize_current(app, SplitDir::Horizontal, -5),
            KeyCode::Down => resize_current(app, SplitDir::Horizontal, 5),
            _ => {}
        }
        return false;
    }

    // 3) Reset layout (no modifier): =
    if press_or_repeat && key.code == KeyCode::Char('=') {
        app.layout = state::LayoutState::default();
        return false;
    }

    // 4) Scrolling (arrows and PageUp/Down)
    if !press_or_repeat {
        return false;
    }
    match key.code {
        KeyCode::Up => scroll_focus(app, -1),
        KeyCode::Down => scroll_focus(app, 1),
        KeyCode::PageUp => scroll_focus(app, -8),
        KeyCode::PageDown => scroll_focus(app, 8),
        _ => {}
    }

    false
}

#[derive(Copy, Clone)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

fn move_focus_dir(app: &mut AppState, dir: Direction) {
    use state::PaneId::*;
    app.focus = match (app.focus, dir) {
        // Top row: Source | VmCanvas
        (Source, Direction::Right) => VmCanvas,
        (Source, Direction::Down) => Symbols,
        (VmCanvas, Direction::Left) => Source,
        (VmCanvas, Direction::Down) => Detail,

        // Bottom row: Symbols | Detail
        (Symbols, Direction::Up) => Source,
        (Symbols, Direction::Right) => Detail,
        (Detail, Direction::Up) => VmCanvas,
        (Detail, Direction::Left) => Symbols,

        // No change for invalid directions
        (cur, _) => cur,
    };
}

fn resize_current(app: &mut AppState, dir: SplitDir, delta: i8) {
    adjust_ratio_recursive(&mut app.layout.root, app.focus, dir, delta);
}

fn adjust_ratio_recursive(node: &mut PaneNode, target: Focus, dir: SplitDir, delta: i8) -> bool {
    match node {
        PaneNode::Leaf(id) => *id == target,
        PaneNode::Split {
            dir: my_dir,
            ratio,
            first,
            second,
        } => {
            let first_has = adjust_ratio_recursive(first, target, dir, delta);
            let second_has = adjust_ratio_recursive(second, target, dir, delta);

            if first_has || second_has {
                if *my_dir == dir {
                    let r = *ratio as i16 + delta as i16;
                    *ratio = r.clamp(20, 80) as u8;
                }
                true
            } else {
                false
            }
        }
    }
}

fn scroll_focus(app: &mut AppState, delta: i16) {
    use state::PaneId;
    match app.focus {
        PaneId::Source => {
            let max = max_scroll(&app.source.lines);
            app.source.scroll_y = apply_scroll(app.source.scroll_y, delta, max);
        }
        PaneId::Symbols => {
            if app.symbols.lines.is_empty() {
                return;
            }
            let len = app.symbols.lines.len() as i32;
            let new_sel = (app.symbols.selected as i32 + delta as i32).clamp(0, len - 1);
            app.symbols.selected = new_sel as usize;
            let max = max_scroll(&app.symbols.lines);
            app.symbols.scroll_y = apply_scroll(app.symbols.scroll_y, delta, max);
        }
        PaneId::VmCanvas => {
            let max = max_scroll(&app.vm.lines);
            app.vm.scroll_y = apply_scroll(app.vm.scroll_y, delta, max);
        }
        PaneId::Detail => {
            let max = max_scroll(&app.detail.lines);
            app.detail.scroll_y = apply_scroll(app.detail.scroll_y, delta, max);
        }
    }
}

fn max_scroll(lines: &[String]) -> u16 {
    lines.len().saturating_sub(1) as u16
}

fn apply_scroll(current: u16, delta: i16, max: u16) -> u16 {
    let new_val = current as i32 + delta as i32;
    new_val.clamp(0, max as i32) as u16
}
