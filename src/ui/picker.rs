//! The add-PR picker overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::{centered, pane};
use crate::app::{App, Picker};

pub fn render(app: &App, picker: &Picker, frame: &mut Frame, area: Rect) {
    let area = centered(area, 80, 70);
    frame.render_widget(Clear, area);
    let block = pane(format!(" Add a PR to {} ", picker.repo), true);
    let items = match &picker.items {
        None => return frame.render_widget(Paragraph::new("Loading open PRs…").block(block), area),
        Some(Err(e)) => {
            let error = Paragraph::new(format!("Cannot list open PRs: {e}"))
                .style(Style::new().fg(Color::Red))
                .block(block);
            return frame.render_widget(error, area);
        }
        Some(Ok(items)) if items.is_empty() => {
            return frame.render_widget(Paragraph::new("No open PRs.").block(block), area);
        }
        Some(Ok(items)) => items,
    };
    let rows: Vec<ListItem> = items
        .iter()
        .map(|pr| {
            let tracked = app.is_tracked(&picker.repo, pr.number);
            let mark = if tracked {
                Span::styled("✓ ", Style::new().fg(Color::Green))
            } else {
                Span::raw("  ")
            };
            let mut spans = vec![
                mark,
                Span::raw(format!("#{} {}", pr.number, pr.title)),
                Span::styled(format!("  {}", pr.author), Style::new().fg(Color::DarkGray)),
            ];
            if pr.is_draft {
                spans.push(Span::styled(" [draft]", Style::new().fg(Color::DarkGray)));
            }
            let style = if tracked {
                Style::new().fg(Color::DarkGray)
            } else {
                Style::new()
            };
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();
    let list = List::new(rows)
        .block(block)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, area, &mut state);
}
