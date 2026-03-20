use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
}

pub fn poll_event(timeout: Duration) -> Option<AppEvent> {
    if event::poll(timeout).ok()? {
        if let Event::Key(key) = event::read().ok()? {
            return Some(AppEvent::Key(key));
        }
    }
    Some(AppEvent::Tick)
}

pub fn should_quit(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

pub fn toggle_mining(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('m'))
}

pub fn next_tab(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Tab)
}
