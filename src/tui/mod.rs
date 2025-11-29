use crate::logger::log_debug;
use crate::mi::Result;
use crate::symbols::SymbolIndexMode;
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

use crate::mi::MiSession;
use state::{AppState, PaneId, SymbolSection};
use std::path::PathBuf;

pub fn run_tui(
    gdb_bin: &str,
    target: &str,
    args: &[String],
    verbose: bool,
    symbol_index_mode: SymbolIndexMode,
    target_basename: Option<String>,
) -> Result<()> {
    // Initialize gdb session
    let mut session = MiSession::start(
        gdb_bin,
        target,
        args,
        verbose,
        symbol_index_mode,
        target_basename.clone(),
    )?;
    session.drain_initial_output()?;

    // Run to main and initialize session state
    let initial_stop = session.run_to_main()?;
    session.ensure_word_size();
    session.ensure_arch();
    session.ensure_endian();

    // Build symbol index once (best effort)
    let symbol_index =
        match session.build_symbol_index(symbol_index_mode, target_basename.as_deref()) {
            Ok(idx) => Some(idx),
            Err(e) => {
                log_debug(&format!("[sym] build_symbol_index failed: {:?}", e));
                None
            }
        };

    // Setup terminal
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    enable_raw_mode()?;
    if let Err(e) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
        disable_raw_mode().ok();
        session.shutdown();
        return Err(e.into());
    }
    let keyboard_enhanced = enable_keyboard_enhancement(terminal.backend_mut());

    // Create app state with session
    let mut app = AppState::new(
        session,
        PathBuf::from(target),
        symbol_index,
        symbol_index_mode,
        verbose,
    );

    // Refresh after initial stop at main
    if let Err(e) = app.refresh_after_stop(Some(&initial_stop)) {
        log_debug(&format!("[tui] refresh_after_stop error: {:?}", e));
    }

    let result = event_loop(&mut terminal, &mut app);

    // Cleanup
    app.debugger.shutdown();
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

    // 2) Escape: Close popup if open
    if press_or_repeat && key.code == KeyCode::Esc {
        if app.show_symbols_popup && app.focus == PaneId::Symbols {
            app.show_symbols_popup = false;
            app.focus = app.last_main_focus;
            return false;
        }
    }

    // 3) Symbols panel: quick switch between locals/globals with 'l' / 'g'
    if press_or_repeat && key.modifiers.is_empty() && matches!(app.focus, PaneId::Symbols) {
        match key.code {
            KeyCode::Char('l') => {
                app.symbols.selected_section = SymbolSection::Locals;
                app.symbols.selected_index = 0;
                clamp_symbol_selection(app);
                return false;
            }
            KeyCode::Char('g') => {
                app.symbols.selected_section = SymbolSection::Globals;
                app.symbols.selected_index = 0;
                clamp_symbol_selection(app);
                return false;
            }
            _ => {}
        }
    }

    // 4) Ctrl + h/l/s for focus movement and popup toggle
    if press_or_repeat && key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('h') => {
                // Focus Source
                app.focus = PaneId::Source;
                app.last_main_focus = PaneId::Source;
                return false;
            }
            KeyCode::Char('l') => {
                // Focus VM
                app.focus = PaneId::VmCanvas;
                app.last_main_focus = PaneId::VmCanvas;
                return false;
            }
            KeyCode::Char('s') => {
                // Toggle Symbols popup
                if app.show_symbols_popup {
                    // Close popup
                    app.show_symbols_popup = false;
                    app.focus = app.last_main_focus;
                } else {
                    // Open popup
                    app.show_symbols_popup = true;
                    app.last_main_focus = app.focus;
                    app.focus = PaneId::Symbols;
                }
                return false;
            }
            KeyCode::Left => {
                // If Symbols popup is focused, adjust popup width
                // Otherwise, adjust main split
                if app.focus == PaneId::Symbols && app.show_symbols_popup {
                    app.adjust_symbols_popup_width(5); // Expand left
                } else {
                    app.adjust_main_split(-5);
                }
                return false;
            }
            KeyCode::Right => {
                // If Symbols popup is focused, adjust popup width
                // Otherwise, adjust main split
                if app.focus == PaneId::Symbols && app.show_symbols_popup {
                    app.adjust_symbols_popup_width(-5); // Shrink right
                } else {
                    app.adjust_main_split(5);
                }
                return false;
            }
            _ => {}
        }
    }

    // 5) F5: Step over (next) -- ignore key repeats to avoid skipping lines.
    if matches!(key.kind, KeyEventKind::Press) && key.code == KeyCode::F(5) {
        match app.debugger.exec_next() {
            Ok(loc) => {
                if let Err(e) = app.refresh_after_stop(Some(&loc)) {
                    log_debug(&format!("[tui] refresh_after_stop error: {:?}", e));
                    return true; // exit TUI when program ended or gdb errored
                }
            }
            Err(e) => {
                log_debug(&format!("[tui] exec_next error: {:?}", e));
                return true; // exit TUI when execution is over or gdb errored
            }
        }
        return false;
    }

    // 6) Scrolling (arrows and PageUp/Down)
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

fn clamp_symbol_selection(app: &mut AppState) {
    let len = match app.symbols.selected_section {
        SymbolSection::Locals => app.symbols.locals.len(),
        SymbolSection::Globals => app.symbols.globals.len(),
    };
    if len == 0 {
        app.symbols.selected_index = 0;
    } else if app.symbols.selected_index >= len {
        app.symbols.selected_index = len - 1;
    }
}

fn scroll_focus(app: &mut AppState, delta: i16) {
    match app.focus {
        PaneId::Source => {
            let max = max_scroll(&app.source.lines) as u32;
            app.source.scroll_top = apply_scroll_u32(app.source.scroll_top, delta, max);
        }
        PaneId::Symbols => {
            let current_len = match app.symbols.selected_section {
                SymbolSection::Locals => app.symbols.locals.len(),
                SymbolSection::Globals => app.symbols.globals.len(),
            };
            if current_len == 0 {
                return;
            }
            let max_index = current_len.saturating_sub(1);
            let new_index = (app.symbols.selected_index as i32 + delta as i32)
                .clamp(0, max_index as i32) as usize;
            app.symbols.selected_index = new_index;
        }
        PaneId::VmCanvas => {
            let max = max_scroll(&app.vm.lines);
            app.vm.scroll_y = apply_scroll(app.vm.scroll_y, delta, max);
        }
        PaneId::Detail => {
            // Detail panel is not rendered in the new layout, but keep for compatibility
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

fn apply_scroll_u32(current: u32, delta: i16, max: u32) -> u32 {
    let new_val = current as i32 + delta as i32;
    new_val.clamp(0, max as i32) as u32
}
