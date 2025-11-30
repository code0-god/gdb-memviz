use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

use crate::tui::state::PaneId;

/// Represents a key combination
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl Key {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn from_event(event: &KeyEvent) -> Self {
        Self {
            code: event.code,
            modifiers: event.modifiers,
        }
    }

    pub fn simple(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::empty(),
        }
    }

    pub fn ctrl(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::CONTROL,
        }
    }

    /// Get a display string for this key (e.g., "Ctrl+h", "q", "F5")
    pub fn display(&self) -> String {
        let key_str = match &self.code {
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{}", n),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            _ => format!("{:?}", self.code),
        };

        if self.modifiers.contains(KeyModifiers::CONTROL) {
            format!("Ctrl+{}", key_str)
        } else if self.modifiers.contains(KeyModifiers::ALT) {
            format!("Alt+{}", key_str)
        } else if self.modifiers.contains(KeyModifiers::SHIFT) {
            format!("Shift+{}", key_str)
        } else {
            key_str
        }
    }
}

/// Actions that can be triggered by key presses
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // App control
    Quit,

    // Navigation
    FocusSource,
    FocusVmCanvas,
    ToggleSymbolsPopup,
    ClosePopup,

    // Symbol panel
    SwitchToLocals,
    SwitchToGlobals,

    // Layout adjustment
    AdjustSplitLeft,
    AdjustSplitRight,

    // Debugging
    StepOver,

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
}

impl Action {
    /// Get a short description of this action for display in UI hints
    pub fn description(&self) -> &'static str {
        match self {
            Action::Quit => "quit",
            Action::FocusSource => "focus source",
            Action::FocusVmCanvas => "focus vm",
            Action::ToggleSymbolsPopup => "symbols",
            Action::ClosePopup => "close",
            Action::SwitchToLocals => "locals",
            Action::SwitchToGlobals => "globals",
            Action::AdjustSplitLeft => "split left",
            Action::AdjustSplitRight => "split right",
            Action::StepOver => "next",
            Action::ScrollUp => "scroll up",
            Action::ScrollDown => "scroll down",
            Action::ScrollPageUp => "page up",
            Action::ScrollPageDown => "page down",
        }
    }
}

/// KeyMap manages key bindings for the TUI
pub struct KeyMap {
    /// Global key bindings (apply in all contexts)
    global: HashMap<Key, Action>,
    /// Context-specific key bindings (apply only when certain pane is focused)
    context_specific: HashMap<PaneId, HashMap<Key, Action>>,
}

impl KeyMap {
    /// Create a new KeyMap with default bindings
    pub fn new() -> Self {
        let mut keymap = KeyMap {
            global: HashMap::new(),
            context_specific: HashMap::new(),
        };
        keymap.init_default_bindings();
        keymap
    }

    /// Initialize default key bindings
    fn init_default_bindings(&mut self) {
        // Global bindings (work in any context)
        self.bind_global(Key::simple(KeyCode::Char('q')), Action::Quit);
        self.bind_global(Key::ctrl(KeyCode::Char('c')), Action::Quit);
        self.bind_global(Key::simple(KeyCode::Esc), Action::ClosePopup);

        // Focus navigation
        // WASD-style (Ctrl-mod) plus existing hjkl variants
        self.bind_global(Key::ctrl(KeyCode::Char('a')), Action::FocusSource);
        self.bind_global(Key::ctrl(KeyCode::Char('d')), Action::FocusVmCanvas);
        self.bind_global(Key::ctrl(KeyCode::Char('s')), Action::ToggleSymbolsPopup);

        // Layout adjustment
        self.bind_global(Key::ctrl(KeyCode::Left), Action::AdjustSplitLeft);
        self.bind_global(Key::ctrl(KeyCode::Right), Action::AdjustSplitRight);

        // Debugging
        self.bind_global(Key::simple(KeyCode::Char('n')), Action::StepOver);

        // Scrolling (global)
        self.bind_global(Key::simple(KeyCode::Up), Action::ScrollUp);
        self.bind_global(Key::simple(KeyCode::Down), Action::ScrollDown);
        self.bind_global(Key::simple(KeyCode::PageUp), Action::ScrollPageUp);
        self.bind_global(Key::simple(KeyCode::PageDown), Action::ScrollPageDown);

        // Symbols panel specific bindings
        self.bind_context(
            PaneId::Symbols,
            Key::simple(KeyCode::Char('l')),
            Action::SwitchToLocals,
        );
        self.bind_context(
            PaneId::Symbols,
            Key::simple(KeyCode::Char('g')),
            Action::SwitchToGlobals,
        );
    }

    /// Bind a key to an action globally
    pub fn bind_global(&mut self, key: Key, action: Action) {
        self.global.insert(key, action);
    }

    /// Bind a key to an action for a specific context
    pub fn bind_context(&mut self, context: PaneId, key: Key, action: Action) {
        self.context_specific
            .entry(context)
            .or_insert_with(HashMap::new)
            .insert(key, action);
    }

    /// Get the action for a key in a given context
    /// First checks context-specific bindings, then falls back to global bindings
    pub fn get_action(&self, key: &Key, context: PaneId) -> Option<&Action> {
        self.context_specific
            .get(&context)
            .and_then(|map| map.get(key))
            .or_else(|| self.global.get(key))
    }

    /// Get all bindings for a specific context (including global)
    pub fn get_bindings_for_context(&self, context: PaneId) -> Vec<(Key, Action)> {
        let mut bindings = Vec::new();

        // Add global bindings
        for (key, action) in &self.global {
            bindings.push((key.clone(), action.clone()));
        }

        // Add context-specific bindings (may override global)
        if let Some(context_map) = self.context_specific.get(&context) {
            for (key, action) in context_map {
                bindings.push((key.clone(), action.clone()));
            }
        }

        bindings
    }

    /// Get key hints for display in the status bar
    /// Returns a list of (key_display, action_description) pairs for important actions
    /// Multiple keys for the same action are joined with " | "
    pub fn get_status_hints(&self) -> Vec<(String, String)> {
        let important_actions = vec![
            Action::FocusSource,
            Action::FocusVmCanvas,
            Action::ToggleSymbolsPopup,
            Action::StepOver,
            Action::Quit,
        ];

        let mut hints = Vec::new();

        for action in important_actions {
            // Special handling for focus keys - combine them into one hint
            if action == Action::FocusSource {
                let mut focus_keys = Vec::new();

                // Find all keys for FocusSource
                for (key, a) in &self.global {
                    if *a == Action::FocusSource {
                        focus_keys.push(key.display().replace("Ctrl+", ""));
                    }
                }

                // Find all keys for FocusVmCanvas
                for (key, a) in &self.global {
                    if *a == Action::FocusVmCanvas {
                        focus_keys.push(key.display().replace("Ctrl+", ""));
                    }
                }

                if !focus_keys.is_empty() {
                    // Sort for consistent ordering
                    focus_keys.sort();
                    let combined = focus_keys.join(" | ");
                    hints.push((format!("Ctrl+{}", combined), "focus".to_string()));
                }
                continue;
            }

            // Skip FocusVmCanvas since it's already handled above
            if action == Action::FocusVmCanvas {
                continue;
            }

            // Find all keys bound to this action
            let mut keys: Vec<String> = self
                .global
                .iter()
                .filter(|(_, a)| **a == action)
                .map(|(k, _)| k.display())
                .collect();

            if !keys.is_empty() {
                // Sort for consistent ordering
                keys.sort();
                let keys_display = keys.join(" | ");
                let description = action.description().to_string();
                hints.push((keys_display, description));
            }
        }

        hints
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_creation() {
        let key1 = Key::simple(KeyCode::Char('q'));
        assert_eq!(key1.code, KeyCode::Char('q'));
        assert!(key1.modifiers.is_empty());

        let key2 = Key::ctrl(KeyCode::Char('c'));
        assert_eq!(key2.code, KeyCode::Char('c'));
        assert!(key2.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_global_binding() {
        let keymap = KeyMap::new();
        let quit_key = Key::simple(KeyCode::Char('q'));

        assert_eq!(
            keymap.get_action(&quit_key, PaneId::Source),
            Some(&Action::Quit)
        );
    }

    #[test]
    fn test_context_specific_binding() {
        let keymap = KeyMap::new();
        let locals_key = Key::simple(KeyCode::Char('l'));

        // Should work in Symbols context
        assert_eq!(
            keymap.get_action(&locals_key, PaneId::Symbols),
            Some(&Action::SwitchToLocals)
        );

        // Should not work in other contexts (no global binding for 'l')
        assert_eq!(keymap.get_action(&locals_key, PaneId::Source), None);
    }
}
