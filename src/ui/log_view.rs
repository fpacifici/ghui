//! Full-screen log window for a check.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::{elapsed, pane};
use crate::github::CheckKind;
use crate::log_view::{LOG_POLL, LogContent, LogView};

/// The window covers everything but the footer.
pub fn area(frame: Rect) -> Rect {
    Rect {
        height: frame.height.saturating_sub(1),
        ..frame
    }
}

/// Visible log lines for a terminal of `size`; the main loop hands this to
/// the app so scrolling can clamp.
pub fn height(size: Size) -> usize {
    usize::from(
        area(Rect::from((ratatui::layout::Position::ORIGIN, size)))
            .height
            .saturating_sub(2),
    )
}

pub fn render(view: &LogView, now: DateTime<Utc>, frame: &mut Frame, frame_area: Rect) {
    let area = area(frame_area);
    frame.render_widget(Clear, area);

    let check = &view.check;
    let end = check.completed_at.unwrap_or(now);
    let mut title = format!(" {} {}", check.state.glyph(), check.name);
    if let Some(workflow) = &check.workflow {
        title.push_str(&format!(" · {workflow}"));
    }
    let duration = elapsed(check.started_at, end);
    if !duration.is_empty() {
        title.push_str(&format!(" · {duration}"));
    }
    title.push(' ');

    let block = pane(title, true);
    let height = usize::from(area.height.saturating_sub(2));
    let body = match &view.content {
        LogContent::Lines(lines) => {
            let top = view.top(height);
            let position = if view.is_following() && check.is_running() {
                " following ".to_string()
            } else {
                let last = (top + height).min(lines.len());
                format!(" {}–{} / {} ", (top + 1).min(last), last, lines.len())
            };
            let block = block.title_bottom(Line::from(position).right_aligned());
            let visible: Vec<Line> = lines
                .iter()
                .skip(top)
                .take(height)
                .map(|l| styled(l))
                .collect();
            return frame.render_widget(Paragraph::new(visible).block(block), area);
        }
        LogContent::Loading => Text::from("Loading log…"),
        LogContent::NotYet => Text::from(format!(
            "Job in progress — logs appear when it completes. Checking every {} s…",
            LOG_POLL.num_seconds()
        )),
        LogContent::Unavailable => {
            Text::from("No log available for this job (it may have expired).")
        }
        LogContent::Error(e) => Text::from(format!("Cannot load the log: {e}")).red(),
        LogContent::Summary => summary(view),
    };
    let paragraph = Paragraph::new(body).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Highlights the workflow commands GitHub embeds in logs.
fn styled(line: &str) -> Line<'_> {
    if let Some(rest) = line.strip_prefix("##[error]") {
        Line::from(vec![Span::raw("Error: "), Span::raw(rest)])
            .style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else if let Some(rest) = line.strip_prefix("##[warning]") {
        Line::from(vec![Span::raw("Warning: "), Span::raw(rest)]).yellow()
    } else if let Some(rest) = line.strip_prefix("##[group]") {
        Line::from(vec![Span::raw("▸ "), Span::raw(rest)])
            .cyan()
            .bold()
    } else if line.starts_with("[command]") {
        Line::from(line).dark_gray()
    } else {
        Line::from(line)
    }
}

/// Non-Actions checks have no log; show what the check reports instead.
fn summary(view: &LogView) -> Text<'static> {
    let mut text = Text::default();
    match &view.check.kind {
        CheckKind::CheckRun { title, summary } => {
            if let Some(title) = title {
                text.push_line(Line::from(title.clone()).bold());
                text.push_line(Line::default());
            }
            if let Some(summary) = summary {
                text.extend(Text::raw(summary.clone()));
            }
        }
        CheckKind::Status { description } => {
            if let Some(description) = description {
                text.push_line(Line::from(description.clone()));
            }
        }
        CheckKind::ActionsJob { .. } => {}
    }
    if text.lines.is_empty() {
        text.push_line(Line::from("This check has no log or summary.").dark_gray());
    }
    text.push_line(Line::default());
    match &view.check.url {
        Some(url) => {
            text.push_line(Line::from(format!("Details: {url}")).blue());
            text.push_line(Line::from("Press o to open it in the browser.").dark_gray());
        }
        None => text.push_line(Line::from("No details URL.").dark_gray()),
    }
    text
}
