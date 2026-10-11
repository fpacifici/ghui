//! Full-screen window listing the session's GitHub API calls; see spec 0009.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::{log_view, pane};
use crate::api_log::{ApiCall, ApiLog};
use crate::scroll::Scroll;

pub fn render(log: &ApiLog, scroll: &Scroll, frame: &mut Frame, frame_area: Rect) {
    // Same size as the check log window.
    let area = log_view::area(frame_area);
    frame.render_widget(Clear, area);
    let block = pane(format!(" API calls ({}) · UTC ", log.len()), true);
    if log.len() == 0 {
        let empty = Paragraph::new("No API calls yet.").block(block);
        return frame.render_widget(empty, area);
    }

    let height = usize::from(area.height.saturating_sub(2));
    let top = scroll.top(log.len(), height);
    let visible: Vec<&ApiCall> = log.iter().skip(top).take(height).collect();
    let last = top + visible.len();
    let position = format!(" {}–{} / {} ", top + 1, last, log.len());
    let block = block.title_bottom(Line::from(position).right_aligned());
    // Align the columns over the visible rows only.
    let path_width = visible.iter().map(|c| c.request.path.len()).max();
    let what_width = visible.iter().map(|c| c.request.what.len()).max();
    let lines: Vec<Line> = visible
        .iter()
        .map(|call| row(call, path_width.unwrap_or(0), what_width.unwrap_or(0)))
        .collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// `12:03:04  POST /graphql  PRs o/a #1  ok`
fn row(call: &ApiCall, path_width: usize, what_width: usize) -> Line<'static> {
    let request = &call.request;
    let (outcome, color) = match &call.outcome {
        Ok(()) => ("ok".to_string(), Color::Green),
        Err(code) => (code.clone(), Color::Red),
    };
    Line::from(vec![
        Span::styled(
            format!("{}  ", call.at.format("%H:%M:%S")),
            Style::new().fg(Color::DarkGray),
        ),
        Span::raw(format!(
            "{:<4} {:<path_width$}  {:<what_width$}  ",
            request.method, request.path, request.what
        )),
        Span::styled(outcome, Style::new().fg(color)),
    ])
}
