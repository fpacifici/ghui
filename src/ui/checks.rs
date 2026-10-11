//! Bottom of the right side: checks, styled after `gh pr checks`.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

use super::{elapsed, pane};
use crate::github::{CheckState, PrDetail};

pub fn render(
    detail: Option<&PrDetail>,
    selected: usize,
    focused: bool,
    now: DateTime<Utc>,
    frame: &mut Frame,
    area: Rect,
) {
    let block = pane(" Checks ".into(), focused);
    let Some(detail) = detail else {
        return frame.render_widget(block, area);
    };
    if detail.checks.is_empty() {
        let empty = Paragraph::new("No checks reported for the head commit.").block(block);
        return frame.render_widget(empty, area);
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [summary_area, _, table_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    frame.render_widget(summary(detail), summary_area);

    let rows = detail.checks.iter().map(|check| {
        let end = check.completed_at.unwrap_or(now);
        Row::new(vec![
            Cell::from(Span::styled(check.state.glyph(), style(check.state))),
            Cell::from(check.name.clone()),
            Cell::from(check.workflow.clone().unwrap_or_default()),
            Cell::from(elapsed(check.started_at, end)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Fill(2),
            Constraint::Fill(1),
            Constraint::Length(8),
        ],
    )
    .column_spacing(2)
    .row_highlight_style(if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    });
    let mut state = TableState::default().with_selected(Some(selected));
    frame.render_stateful_widget(table, table_area, &mut state);
}

/// `1 failing, 2 pending, 7 successful, 0 skipped`.
fn summary(detail: &PrDetail) -> Line<'static> {
    let count = |state| detail.checks.iter().filter(|c| c.state == state).count();
    let parts = [
        (CheckState::Fail, "failing"),
        (CheckState::Pending, "pending"),
        (CheckState::Pass, "successful"),
        (CheckState::Skipped, "skipped"),
    ];
    let mut spans = Vec::new();
    for (i, (state, label)) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(", "));
        }
        let n = count(state);
        let style = if n > 0 { style(state) } else { Style::new() };
        spans.push(Span::styled(format!("{n} {label}"), style));
    }
    Line::from(spans)
}

fn style(state: CheckState) -> Style {
    match state {
        CheckState::Pass => Style::new().fg(Color::Green),
        CheckState::Fail => Style::new().fg(Color::Red),
        CheckState::Pending => Style::new().fg(Color::Yellow),
        CheckState::Skipped => Style::new().fg(Color::DarkGray),
    }
}
