//! Bottom of the right side: checks, styled after `gh pr checks`.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

use super::{elapsed, pane};
use crate::app::RepoState;
use crate::check_tree::{Node, Row as TreeRow};
use crate::github::{CheckState, PrDetail};

pub fn render(repo: &RepoState, focused: bool, now: DateTime<Utc>, frame: &mut Frame, area: Rect) {
    let block = pane(" Checks ".into(), focused);
    let Some(detail) = repo.selected_detail() else {
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

    let rows = repo
        .check_rows()
        .iter()
        .map(|row| table_row(row, now))
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Fill(3),
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
    let mut state = TableState::default().with_selected(Some(repo.selected_check));
    frame.render_stateful_widget(table, table_area, &mut state);
}

/// `  ▾ * name   detail   elapsed`: indented by depth, with a fold marker
/// on runs and jobs that have steps.
fn table_row(row: &TreeRow, now: DateTime<Utc>) -> Row<'static> {
    let (state, name, detail, started, completed) = match &row.node {
        Node::Run(run) => {
            let jobs = match run.jobs.len() {
                1 => "1 job".to_string(),
                n => format!("{n} jobs"),
            };
            let (start, end) = (run.started_at(), run.completed_at());
            (run.state(), run.name.to_string(), jobs, start, end)
        }
        Node::Check(check) => {
            // Jobs under a run already show the workflow on the run row.
            let workflow = if row.depth == 0 {
                check.workflow.clone().unwrap_or_default()
            } else {
                String::new()
            };
            let (start, end) = (check.started_at, check.completed_at);
            (check.state, check.name.clone(), workflow, start, end)
        }
        Node::Step { step, .. } => {
            let (start, end) = (step.started_at, step.completed_at);
            (step.state, step.name.clone(), String::new(), start, end)
        }
    };
    let marker = match row.fold {
        Some((_, true)) => "▾ ",
        Some((_, false)) => "▸ ",
        None => "  ",
    };
    let tree = Line::from(vec![
        Span::raw(format!("{}{marker}", "  ".repeat(row.depth))),
        Span::styled(state.glyph(), style(state)),
        Span::raw(format!(" {name}")),
    ]);
    // A pending step that has not started has no time yet.
    let end = completed.unwrap_or(now);
    Row::new(vec![
        Cell::from(tree),
        Cell::from(detail),
        Cell::from(elapsed(started, end)),
    ])
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
