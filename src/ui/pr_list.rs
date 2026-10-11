//! Left column: the tracked PRs of the active repo.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::pane;
use crate::app::{PrEntry, RepoState};
use crate::github::Rollup;

pub fn render(repo: &RepoState, focused: bool, frame: &mut Frame, area: Rect) {
    let block = pane(" PRs ".into(), focused);
    if repo.prs.is_empty() {
        let hint = Paragraph::new("No PRs tracked.\nCtrl+A to add one.").block(block);
        return frame.render_widget(hint, area);
    }
    let items: Vec<ListItem> = repo.prs.iter().map(|pr| ListItem::new(row(pr))).collect();
    let highlight = if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    let list = List::new(items).block(block).highlight_style(highlight);
    let mut state = ListState::default().with_selected(Some(repo.selected_pr));
    frame.render_stateful_widget(list, area, &mut state);
}

/// `● #123 Title`, the dot colored by the aggregate check state.
fn row(pr: &PrEntry) -> Line<'static> {
    let number = format!("#{}", pr.number);
    let Some(detail) = &pr.detail else {
        let (dot, text) = match &pr.error {
            Some(e) => (Span::styled("! ", Style::new().fg(Color::Red)), e.clone()),
            None => (Span::raw("  "), "loading…".to_string()),
        };
        return Line::from(vec![dot, Span::raw(format!("{number} {text}"))]);
    };
    let color = match detail.rollup() {
        Rollup::Success => Color::Green,
        Rollup::Failure => Color::Red,
        Rollup::Pending => Color::Yellow,
        Rollup::None => Color::DarkGray,
    };
    Line::from(vec![
        Span::styled("● ", Style::new().fg(color)),
        Span::raw(format!("{number} {}", detail.title)),
    ])
}
