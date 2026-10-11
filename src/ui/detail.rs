//! Top third of the right side: PR header and raw description.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Wrap};

use super::pane;
use crate::app::PrEntry;
use crate::github::PrState;

pub fn render(pr: Option<&PrEntry>, frame: &mut Frame, area: Rect) {
    let Some(pr) = pr else {
        return frame.render_widget(pane(" Description ".into(), false), area);
    };
    let Some(detail) = &pr.detail else {
        let message = pr.error.clone().unwrap_or_else(|| "Loading…".into());
        let block = pane(format!(" #{} ", pr.number), false);
        return frame.render_widget(Paragraph::new(message).block(block), area);
    };

    let state = match detail.state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    };
    let mut text = Text::from(vec![
        Line::from(format!(
            "{} · {} → {} · {state}",
            detail.author, detail.head, detail.base
        ))
        .dark_gray(),
        Line::from(detail.url.clone()).underlined().blue(),
        Line::default(),
    ]);
    if detail.body.trim().is_empty() {
        text.push_line(Line::from("No description provided.").dark_gray().italic());
    } else {
        // Raw markdown, as agreed: no rendering, just wrapping.
        text.extend(Text::raw(detail.body.replace('\r', "")));
    }
    let block = pane(format!(" #{} {} ", detail.number, detail.title), false);
    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
