//! Rendering. Reads [`App`] state and draws it; never mutates it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};

use crate::app::{App, Tab};

pub fn render(app: &App, frame: &mut Frame) {
    let [tabs_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let titles = Tab::ALL.iter().map(|t| t.title());
    let selected_tab = Tab::ALL.iter().position(|t| *t == app.tab);
    let tabs = Tabs::new(titles)
        .block(Block::bordered().title(" ghui ".bold()))
        .select(selected_tab)
        .highlight_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, tabs_area);

    let items: Vec<ListItem> = app
        .items
        .iter()
        .map(|s| ListItem::new(s.as_str()))
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(format!(" {} ", app.tab.title())))
        .highlight_style(Style::new().reversed())
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, body_area, &mut state);

    let help = Line::from(" q quit · tab/←→ switch tab · j/k move ").dark_gray();
    frame.render_widget(Paragraph::new(help), footer_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_to_string(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        terminal.draw(|f| render(app, f)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_tabs_list_and_footer() {
        let out = render_to_string(&App::default());
        assert!(out.contains("ghui"));
        assert!(out.contains("Pull Requests"));
        assert!(out.contains("> PR #1"));
        assert!(out.contains("q quit"));
    }
}
