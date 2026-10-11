//! Translates terminal input into application actions.

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Everything the user can ask the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    NextTab,
    PreviousTab,
    Up,
    Down,
}

/// Waits briefly for an input event and maps it to an [`Action`].
pub fn next_action() -> color_eyre::Result<Option<Action>> {
    if !event::poll(Duration::from_millis(250))? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(map_key(key)),
        _ => Ok(None),
    }
}

/// Maps a key press to an action. Kept separate from I/O so it can be tested.
pub fn map_key(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),
        (KeyCode::Char('q') | KeyCode::Esc, _) => Some(Action::Quit),
        (KeyCode::Tab | KeyCode::Right | KeyCode::Char('l'), _) => Some(Action::NextTab),
        (KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h'), _) => Some(Action::PreviousTab),
        (KeyCode::Down | KeyCode::Char('j'), _) => Some(Action::Down),
        (KeyCode::Up | KeyCode::Char('k'), _) => Some(Action::Up),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn maps_core_keys() {
        assert_eq!(map_key(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(map_key(key(KeyCode::Tab)), Some(Action::NextTab));
        assert_eq!(map_key(key(KeyCode::BackTab)), Some(Action::PreviousTab));
        assert_eq!(map_key(key(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(map_key(key(KeyCode::Char('k'))), Some(Action::Up));
        assert_eq!(map_key(key(KeyCode::Char('x'))), None);
    }

    #[test]
    fn ctrl_c_quits() {
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(map_key(ev), Some(Action::Quit));
    }
}
