//! Application state and the main loop.

use ratatui::DefaultTerminal;

use crate::event::{self, Action};
use crate::ui;

/// Top-level sections of the UI, shown as tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    PullRequests,
    Issues,
    Notifications,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::PullRequests, Tab::Issues, Tab::Notifications];

    pub fn title(self) -> &'static str {
        match self {
            Tab::PullRequests => "Pull Requests",
            Tab::Issues => "Issues",
            Tab::Notifications => "Notifications",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> Tab {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Tab {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// All mutable state of the application.
#[derive(Debug)]
pub struct App {
    pub tab: Tab,
    /// Placeholder rows per tab until the GitHub client is wired in.
    pub items: Vec<String>,
    pub selected: usize,
    pub running: bool,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            tab: Tab::default(),
            items: Vec::new(),
            selected: 0,
            running: true,
        };
        app.load_items();
        app
    }
}

impl App {
    /// Runs the draw / handle-event loop until the user quits.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(&self, frame))?;
            if let Some(action) = event::next_action()? {
                self.apply(action);
            }
        }
        Ok(())
    }

    /// Applies a single action to the state. Pure, so it is easy to test.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            Action::NextTab => self.set_tab(self.tab.next()),
            Action::PreviousTab => self.set_tab(self.tab.previous()),
            Action::Down => {
                if !self.items.is_empty() {
                    self.selected = (self.selected + 1).min(self.items.len() - 1);
                }
            }
            Action::Up => self.selected = self.selected.saturating_sub(1),
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.selected = 0;
        self.load_items();
    }

    fn load_items(&mut self) {
        let prefix = match self.tab {
            Tab::PullRequests => "PR",
            Tab::Issues => "Issue",
            Tab::Notifications => "Notification",
        };
        self.items = (1..=5)
            .map(|n| format!("{prefix} #{n} (placeholder)"))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_stops_the_app() {
        let mut app = App::default();
        app.apply(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn tabs_cycle_in_both_directions() {
        let mut app = App::default();
        app.apply(Action::NextTab);
        assert_eq!(app.tab, Tab::Issues);
        app.apply(Action::PreviousTab);
        app.apply(Action::PreviousTab);
        assert_eq!(app.tab, Tab::Notifications);
    }

    #[test]
    fn selection_is_clamped_and_reset_on_tab_change() {
        let mut app = App::default();
        app.apply(Action::Up);
        assert_eq!(app.selected, 0);
        for _ in 0..10 {
            app.apply(Action::Down);
        }
        assert_eq!(app.selected, app.items.len() - 1);
        app.apply(Action::NextTab);
        assert_eq!(app.selected, 0);
    }
}
