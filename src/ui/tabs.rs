//! Repo tab bar. Repos that failed to load are dimmed.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Tabs};

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let titles = app.repos.iter().enumerate().map(|(i, repo)| {
        let style = match (repo.error.is_some(), i == app.active) {
            (true, _) => Style::new().fg(Color::DarkGray),
            (false, true) => Style::new().fg(Color::Cyan),
            (false, false) => Style::new(),
        };
        Line::styled(repo.name.clone(), style)
    });
    // The refresh "button": `r` re-fetches every tab and looks for new PRs.
    let refresh = if app.refreshing {
        Line::from(" ⟳ refreshing… ").yellow()
    } else {
        Line::from(vec![" ⟳ ".into(), "r".bold(), " refresh ".into()])
    };
    let block = Block::bordered()
        .title(" ghui ".bold())
        .title_top(refresh.right_aligned());
    let tabs = Tabs::new(titles)
        .block(block)
        .select(Some(app.active))
        // Style-only highlight so a dimmed tab stays dimmed when selected.
        .highlight_style(Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED));
    frame.render_widget(tabs, area);
}
