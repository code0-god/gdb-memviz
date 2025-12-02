use crate::logger::log_debug;
use crate::mi::Result;
use crate::symbols::SymbolIndexMode;
use crossterm::{
    event::{
        self, Event, KeyEvent, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
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

pub mod highlight;
pub mod keymap;
pub mod state;
pub mod theme;
pub mod ui;

use crate::mi::MiSession;
use keymap::{Action, Key, KeyMap};
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
    let keymap = KeyMap::new();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if debug_keys {
                eprintln!("[tui-ev] {:?}", ev);
            }
            if let Event::Key(key_event) = ev {
                if handle_key(key_event, app, &keymap) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn handle_key(key: KeyEvent, app: &mut AppState, keymap: &KeyMap) -> bool {
    let press_or_repeat = matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat);

    // Special handling for F5 (StepOver): ignore key repeats to avoid skipping lines
    let press_only = matches!(key.kind, KeyEventKind::Press);

    // Get the key representation
    let key_input = Key::from_event(&key);

    // Look up action in keymap based on current context
    if let Some(action) = keymap.get_action(&key_input, app.focus) {
        // Execute the action
        match action {
            Action::Quit => {
                if press_or_repeat {
                    return true;
                }
            }
            Action::ClosePopup => {
                if press_or_repeat {
                    if app.show_symbols_popup && app.focus == PaneId::Symbols {
                        app.show_symbols_popup = false;
                        app.focus = app.last_main_focus;
                    }
                }
            }
            Action::FocusSource => {
                if press_or_repeat {
                    app.focus = PaneId::Source;
                    app.last_main_focus = PaneId::Source;
                }
            }
            Action::FocusVmCanvas => {
                if press_or_repeat {
                    app.focus = PaneId::VmCanvas;
                    app.last_main_focus = PaneId::VmCanvas;
                }
            }
            Action::ToggleSymbolsPopup => {
                if press_or_repeat {
                    if !app.show_symbols_popup {
                        app.show_symbols_popup = true;
                        app.last_main_focus = app.focus;
                        app.focus = PaneId::Symbols;
                    } else {
                        // Already open: just move focus to Symbols without closing
                        app.focus = PaneId::Symbols;
                    }
                }
            }
            Action::SwitchToLocals => {
                if press_or_repeat {
                    app.symbols.selected_section = SymbolSection::Locals;
                    app.symbols.selected_index = 0;
                    clamp_symbol_selection(app);
                }
            }
            Action::SwitchToGlobals => {
                if press_or_repeat {
                    app.symbols.selected_section = SymbolSection::Globals;
                    app.symbols.selected_index = 0;
                    clamp_symbol_selection(app);
                }
            }
            Action::AdjustSplitLeft => {
                if press_or_repeat {
                    if app.focus == PaneId::Symbols && app.show_symbols_popup {
                        app.adjust_symbols_popup_width(5); // Expand left
                    } else {
                        app.adjust_main_split(-5);
                    }
                }
            }
            Action::AdjustSplitRight => {
                if press_or_repeat {
                    if app.focus == PaneId::Symbols && app.show_symbols_popup {
                        app.adjust_symbols_popup_width(-5); // Shrink right
                    } else {
                        app.adjust_main_split(5);
                    }
                }
            }
            Action::StepOver => {
                if press_only {
                    match app.debugger.exec_next() {
                        Ok(loc) => {
                            if let Err(e) = app.refresh_after_stop(Some(&loc)) {
                                log_debug(&format!("[tui] refresh_after_stop error: {:?}", e));
                                return true;
                            }
                        }
                        Err(e) => {
                            log_debug(&format!("[tui] exec_next error: {:?}", e));
                            return true;
                        }
                    }
                }
            }
            Action::ScrollUp => {
                if press_or_repeat {
                    scroll_focus(app, -1);
                }
            }
            Action::ScrollDown => {
                if press_or_repeat {
                    scroll_focus(app, 1);
                }
            }
            Action::ScrollPageUp => {
                if press_or_repeat {
                    scroll_focus(app, -8);
                }
            }
            Action::ScrollPageDown => {
                if press_or_repeat {
                    scroll_focus(app, 8);
                }
            }
        }
    }

    // VmCanvas 포커스 시 추가 키 처리 (좌우, 페이지 업/다운)
    if press_or_repeat && app.focus == PaneId::VmCanvas {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                app.vm_left();
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                app.vm_right();
            }
            (KeyCode::Up, KeyModifiers::CONTROL) => {
                app.vm_page_up();
            }
            (KeyCode::Down, KeyModifiers::CONTROL) => {
                app.vm_page_down();
            }
            _ => {}
        }
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
            // VM hexdump 스크롤 (위/아래)
            if delta < 0 {
                for _ in 0..(-delta) {
                    app.vm_move_up();
                }
            } else {
                for _ in 0..delta {
                    app.vm_move_down();
                }
            }
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
