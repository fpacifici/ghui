//! The add-PR picker overlay; see spec 0008.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::{centered, pane};
use crate::app::{App, Picker};

pub fn render(app: &App, picker: &Picker, frame: &mut Frame, area: Rect) {
    let area = centered(area, 80, 70);
    frame.render_widget(Clear, area);
    let (shown, other) = if picker.mine {
        ("mine", "all")
    } else {
        ("all", "mine")
    };
    let block = pane(
        format!(" Add a PR to {} — {shown} (m: {other}) ", picker.repo),
        true,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [list_area, filter_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    frame.render_widget(filter_bar(picker), filter_area);

    let message = |text: String| Paragraph::new(text);
    let typed = if picker.filter.is_empty() {
        String::new()
    } else {
        format!("\nEnter: track #{} anyway", picker.filter)
    };
    let items = match &picker.items {
        None => {
            return frame.render_widget(message(format!("Loading open PRs…{typed}")), list_area);
        }
        Some(Err(e)) => {
            let error = message(format!("Cannot list open PRs: {e}{typed}")).red();
            return frame.render_widget(error, list_area);
        }
        Some(Ok(_)) => picker.visible(),
    };
    if items.is_empty() {
        let text = if !picker.filter.is_empty() {
            format!("No PR matches #{0}.\nEnter: track #{0}", picker.filter)
        } else if picker.mine {
            "You have no open PRs here. m: show all".into()
        } else {
            "No open PRs.".into()
        };
        return frame.render_widget(message(text), list_area);
    }
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
    let list = List::new(rows).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, list_area, &mut state);
}

/// `#123▏`, or a hint while empty.
fn filter_bar(picker: &Picker) -> Line<'static> {
    if picker.filter.is_empty() {
        return Line::from("# type a PR number to filter").dark_gray();
    }
    Line::from(vec![
        Span::raw(format!("#{}", picker.filter)),
        Span::styled("▏", Style::new().fg(Color::Cyan)),
    ])
}
