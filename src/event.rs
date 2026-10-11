//! Translates terminal input into application actions.

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Everything the user can ask the app to do. Context-free: what an action
/// means (e.g. `Enter` in the checks pane vs. the picker) is decided by
/// [`crate::app::App::apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `Ctrl+C`: quit from anywhere.
    ForceQuit,
    /// `q`: close the modal, or quit.
    Quit,
    /// `Esc`: close the modal.
    Close,
    NextRepo,
    PreviousRepo,
    FocusLeft,
    FocusRight,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Enter,
    /// `Space`: expand or collapse in the checks tree.
    Toggle,
    AddPr,
    /// `Ctrl+D` or `d`: stop tracking the selected PR.
    Untrack,
    Refresh,
    CopyUrl,
    OpenUrl,
    /// `m`: in the picker, switch between the user's PRs and all.
    ToggleMine,
    /// `0`–`9`: in the picker, type into the PR number filter.
    Digit(u8),
    /// `Backspace`: in the picker, delete the last filter digit.
    Backspace,
    /// `L`: open the API call log.
    ApiLog,
}

/// Waits briefly for an input event and maps it to an [`Action`].
pub fn next_action(timeout: Duration) -> color_eyre::Result<Option<Action>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(map_key(key)),
        _ => Ok(None),
    }
}

/// Maps a key press to an action. Kept separate from I/O so it can be tested.
pub fn map_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Some(Action::ForceQuit),
            KeyCode::Char('a') => Some(Action::AddPr),
            KeyCode::Char('d') => Some(Action::Untrack),
            _ => None,
        };
    }
    // Shifted characters ('<', '>', 'G') may or may not carry SHIFT
    // depending on the terminal, so modifiers are ignored from here on.
    let action = match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Esc => Action::Close,
        KeyCode::Char('>') | KeyCode::Tab => Action::NextRepo,
        KeyCode::Char('<') | KeyCode::BackTab => Action::PreviousRepo,
        KeyCode::Left | KeyCode::Char('h') => Action::FocusLeft,
        KeyCode::Right | KeyCode::Char('l') => Action::FocusRight,
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::Home | KeyCode::Char('g') => Action::Top,
        KeyCode::End | KeyCode::Char('G') => Action::Bottom,
        KeyCode::Enter => Action::Enter,
        KeyCode::Char(' ') => Action::Toggle,
        KeyCode::Char('d') => Action::Untrack,
        KeyCode::Char('r') => Action::Refresh,
        KeyCode::Char('y') => Action::CopyUrl,
        KeyCode::Char('o') => Action::OpenUrl,
        KeyCode::Char('m') => Action::ToggleMine,
        KeyCode::Char(c @ '0'..='9') => Action::Digit(c as u8 - b'0'),
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char('L') => Action::ApiLog,
        _ => return None,
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn maps_navigation_keys() {
        assert_eq!(map_key(key(KeyCode::Char('>'))), Some(Action::NextRepo));
        assert_eq!(map_key(key(KeyCode::Char('<'))), Some(Action::PreviousRepo));
        assert_eq!(map_key(key(KeyCode::Tab)), Some(Action::NextRepo));
        assert_eq!(map_key(key(KeyCode::BackTab)), Some(Action::PreviousRepo));
        assert_eq!(map_key(key(KeyCode::Left)), Some(Action::FocusLeft));
        assert_eq!(map_key(key(KeyCode::Char('l'))), Some(Action::FocusRight));
        assert_eq!(map_key(key(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(map_key(key(KeyCode::Up)), Some(Action::Up));
        assert_eq!(map_key(key(KeyCode::Char(' '))), Some(Action::Toggle));
        assert_eq!(map_key(key(KeyCode::Char('x'))), None);
    }

    #[test]
    fn shifted_characters_match_with_or_without_shift() {
        let shifted = KeyEvent::new(KeyCode::Char('>'), KeyModifiers::SHIFT);
        assert_eq!(map_key(shifted), Some(Action::NextRepo));
        let shifted = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(map_key(shifted), Some(Action::Bottom));
    }

    #[test]
    fn control_keys() {
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(map_key(ctrl('c')), Some(Action::ForceQuit));
        assert_eq!(map_key(ctrl('a')), Some(Action::AddPr));
        assert_eq!(map_key(ctrl('d')), Some(Action::Untrack));
        assert_eq!(map_key(key(KeyCode::Char('d'))), Some(Action::Untrack));
        assert_eq!(map_key(ctrl('x')), None);
    }

    #[test]
    fn picker_keys() {
        assert_eq!(map_key(key(KeyCode::Char('m'))), Some(Action::ToggleMine));
        assert_eq!(map_key(key(KeyCode::Char('0'))), Some(Action::Digit(0)));
        assert_eq!(map_key(key(KeyCode::Char('7'))), Some(Action::Digit(7)));
        assert_eq!(map_key(key(KeyCode::Backspace)), Some(Action::Backspace));
    }

    #[test]
    fn capital_l_opens_the_api_log() {
        let shifted = KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT);
        assert_eq!(map_key(shifted), Some(Action::ApiLog));
        assert_eq!(map_key(key(KeyCode::Char('l'))), Some(Action::FocusRight));
    }

    #[test]
    fn esc_closes_and_q_quits() {
        assert_eq!(map_key(key(KeyCode::Esc)), Some(Action::Close));
        assert_eq!(map_key(key(KeyCode::Char('q'))), Some(Action::Quit));
    }
}
