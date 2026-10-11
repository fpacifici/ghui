//! Rendering. Reads [`App`] state and draws it; never mutates it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame) {
    let [tabs_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let tabs = Tabs::new(app.repos.iter().map(|r| r.name.as_str()))
        .block(Block::bordered().title(" ghui ".bold()))
        .select(Some(app.active))
        .highlight_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, tabs_area);

    if let Some(repo) = app.active_repo() {
        let items: Vec<ListItem> = repo
            .prs
            .iter()
            .map(|p| ListItem::new(format!("#{}", p.number)))
            .collect();
        let list = List::new(items)
            .block(Block::bordered())
            .highlight_style(Style::new().reversed());
        let mut state = ListState::default().with_selected(Some(repo.selected_pr));
        frame.render_stateful_widget(list, body_area, &mut state);
    }

    let help = Line::from(" q quit · </> repo · j/k move ").dark_gray();
    frame.render_widget(Paragraph::new(help), footer_area);
}
