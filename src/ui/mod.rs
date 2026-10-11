//! Rendering. Reads [`App`] state and draws it; never mutates it.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::{App, Focus, Modal};

mod checks;
mod detail;
pub mod log_view;
mod picker;
mod pr_list;
mod tabs;

pub fn render(app: &App, frame: &mut Frame) {
    let status_height = u16::from(app.status.is_some());
    let [tabs_area, body_area, status_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(status_height),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    tabs::render(app, frame, tabs_area);
    render_body(app, frame, body_area);
    if let Some(status) = &app.status {
        frame.render_widget(Line::from(format!(" {status}")).red(), status_area);
    }
    frame.render_widget(footer(app), footer_area);

    match &app.modal {
        Some(Modal::Picker(p)) => picker::render(app, p, frame, body_area),
        Some(Modal::Log(view)) => log_view::render(view, app.now, frame, frame.area()),
        None => {}
    }
}

fn render_body(app: &App, frame: &mut Frame, area: Rect) {
    let Some(repo) = app.active_repo() else {
        return render_empty_config(app, frame, area);
    };
    if let Some(error) = &repo.error {
        let text = vec![
            Line::from(format!("Cannot load {}", repo.name)).bold(),
            Line::default(),
            Line::from(error.to_string()),
            Line::default(),
            Line::from("Retrying every 60 s; press r to retry now.").dark_gray(),
        ];
        let paragraph = Paragraph::new(text)
            .red()
            .wrap(Wrap { trim: false })
            .block(Block::bordered());
        return frame.render_widget(paragraph, area);
    }

    let [list_area, right_area] =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).areas(area);
    let [detail_area, checks_area] =
        Layout::vertical([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]).areas(right_area);

    pr_list::render(repo, app.focus == Focus::List, frame, list_area);
    let pr = repo.selected();
    detail::render(pr, frame, detail_area);
    checks::render(
        repo,
        app.focus == Focus::Checks,
        app.now,
        frame,
        checks_area,
    );
}

fn render_empty_config(app: &App, frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::from("No repositories tracked yet.").bold(),
        Line::default(),
        Line::from(format!("Add them to {}:", app.config_path)),
        Line::default(),
        Line::from("  repos:").cyan(),
        Line::from("    - name: owner/repo").cyan(),
        Line::from("      prs: [123]").cyan(),
        Line::default(),
        Line::from("then restart ghui. PRs can also be added with Ctrl+A.").dark_gray(),
    ];
    frame.render_widget(Paragraph::new(text).block(Block::bordered()), area);
}

fn footer(app: &App) -> Line<'static> {
    let keys = match (&app.modal, app.focus) {
        (Some(Modal::Picker(_)), _) => " esc close · j/k move · pgup/pgdn page · enter add",
        (Some(Modal::Log(_)), _) => {
            " esc close · j/k scroll · pgup/pgdn page · g/G top/bottom · r refresh · y copy url · o open"
        }
        (None, Focus::List) => {
            " q quit · </> repo · → checks · j/k move · enter/o open · ^A add · ^D untrack · r refresh · y copy url"
        }
        (None, Focus::Checks) => {
            " q quit · </> repo · ←/→/space fold · j/k move · enter logs/open running · r refresh · y copy url · o open check"
        }
    };
    Line::from(keys).dark_gray()
}

/// A `width_pct` × `height_pct` rectangle centered in `area`.
pub(crate) fn centered(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Percentage(width_pct)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Percentage(height_pct)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    area
}

/// Border style of a pane, highlighted when it has focus.
pub(crate) fn pane(title: String, focused: bool) -> Block<'static> {
    let block = Block::bordered().title(title);
    if focused {
        block.border_style(Style::new().fg(Color::Cyan))
    } else {
        block
    }
}

/// Compact duration in the style of `gh pr checks`: `45s`, `3m12s`, `1h5m`.
pub(crate) fn elapsed(start: Option<DateTime<Utc>>, end: DateTime<Utc>) -> String {
    let Some(start) = start else {
        return String::new();
    };
    let secs = (end - start).num_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{}s", s / 60, s % 60),
        s => format!("{}h{}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::tests::{check, config, detail, job_id, loaded, t};
    use crate::app::{Msg, RepoState};
    use crate::config::Config;
    use crate::event::Action;
    use crate::github::{ApiError, CheckState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    pub fn draw(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(app, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    pub fn text(buffer: &Buffer) -> String {
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The first cell where `needle` starts.
    pub fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
        let text = text(buffer);
        text.lines().enumerate().find_map(|(y, line)| {
            let byte = line.find(needle)?;
            let x = line[..byte].chars().count();
            Some((x as u16, y as u16))
        })
    }

    pub fn sample_app() -> App {
        let mut app = App::new(
            config(&[("o/a", &[1, 2]), ("o/b", &[3])]),
            "cfg".into(),
            t(0),
        );
        let mut one = detail(
            1,
            vec![
                check("lint", CheckState::Fail),
                check("build", CheckState::Pending),
                check("test", CheckState::Pass),
            ],
        );
        one.body = "## Summary\nDoes things.".into();
        let two = detail(2, vec![]);
        app.handle(loaded("o/a", vec![(1, Ok(one)), (2, Ok(two))]), t(0));
        app.now = t(20);
        app
    }

    #[test]
    fn main_view_shows_tabs_list_description_and_checks() {
        let out = text(&draw(&sample_app(), 120, 30));
        assert!(out.contains("o/a"));
        assert!(out.contains("o/b"));
        assert!(out.contains("#1 PR 1"));
        assert!(out.contains("#2 PR 2"));
        assert!(out.contains("me · feature → main · open"));
        assert!(out.contains("https://github.com/o/r/pull/1"));
        assert!(out.contains("## Summary"));
        assert!(out.contains("1 failing, 1 pending, 1 successful, 0 skipped"));
        assert!(out.contains("  X lint"));
        assert!(out.contains("  * build"));
        assert!(out.contains("50s"), "running check elapsed: {out}");
        assert!(out.contains("20s"), "completed check elapsed: {out}");
        assert!(out.contains("q quit"));
    }

    #[test]
    fn checks_render_as_a_tree_of_runs_jobs_and_steps() {
        use crate::github::Step;
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let step = |number, name: &str, state, started: Option<i64>| Step {
            number,
            name: name.into(),
            state,
            started_at: started.map(t),
            completed_at: (state != CheckState::Pending).then(|| t(5)),
        };
        let mut test = check("test", CheckState::Pending);
        test.run_id = Some(7);
        test.steps = vec![
            step(1, "Set up job", CheckState::Pass, Some(0)),
            step(2, "Run tests", CheckState::Pending, Some(5)),
            step(3, "Post checkout", CheckState::Pending, None),
        ];
        let mut lint = check("lint", CheckState::Fail);
        lint.run_id = Some(7);
        lint.steps = vec![step(1, "Clippy", CheckState::Fail, Some(0))];
        let mut status = check("deploy", CheckState::Pass);
        status.workflow = None;
        status.kind = crate::github::CheckKind::Status { description: None };
        app.handle(
            loaded("o/a", vec![(1, Ok(detail(1, vec![lint, test, status])))]),
            t(0),
        );
        app.now = t(20);
        let out = text(&draw(&app, 120, 30));
        let line = |needle: &str| {
            out.lines()
                .find(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no {needle}: {out}"))
                .to_string()
        };
        assert!(line("▾ X CI").contains("2 jobs"), "{out}");
        assert!(line("▾ X CI").contains("50s"), "{out}");
        assert!(line("  ▸ X lint").contains("20s"), "{out}");
        assert!(!out.contains("Clippy"), "{out}");
        assert!(line("  ▾ * test").contains("50s"), "{out}");
        assert!(line("      ✓ Set up job").contains("5s"), "{out}");
        assert!(line("      * Run tests").contains("15s"), "{out}");
        // Not started: no time.
        let post = line("      * Post checkout");
        assert!(!post.chars().any(|c| c.is_ascii_digit()), "{out}");
        assert!(line("  ✓ deploy").contains("20s"), "{out}");
        assert!(out.contains("1 failing, 1 pending, 1 successful"), "{out}");

        // Expanding the failed job shows the failing step.
        app.apply(Action::FocusRight);
        app.apply(Action::Down);
        app.apply(Action::Toggle);
        let out = text(&draw(&app, 120, 30));
        assert!(out.contains("  ▾ X lint"), "{out}");
        assert!(out.contains("      X Clippy"), "{out}");
    }

    #[test]
    fn tab_bar_shows_the_refresh_button() {
        let mut app = sample_app();
        let out = text(&draw(&app, 120, 30));
        assert!(out.lines().next().unwrap().contains("⟳ r refresh"), "{out}");
        app.apply(Action::Refresh);
        let out = text(&draw(&app, 120, 30));
        assert!(out.lines().next().unwrap().contains("refreshing…"), "{out}");
    }

    #[test]
    fn footer_advertises_ctrl_d_untrack_in_the_pr_list_only() {
        let mut app = sample_app();
        let out = text(&draw(&app, 140, 30));
        assert!(out.contains("^D untrack"), "{out}");
        app.apply(Action::FocusRight);
        let out = text(&draw(&app, 140, 30));
        assert!(!out.contains("untrack"), "{out}");
    }

    #[test]
    fn untracked_pr_disappears_from_the_list() {
        let mut app = sample_app();
        app.apply(Action::Untrack);
        let out = text(&draw(&app, 120, 30));
        assert!(!out.contains("#1 PR 1"), "{out}");
        assert!(out.contains("#2 PR 2"), "{out}");
    }

    #[test]
    fn description_takes_the_top_third() {
        let buffer = draw(&sample_app(), 120, 33);
        // Body spans rows 3..32 (29 rows): description gets 10, checks 19.
        let (_, checks_row) = find(&buffer, "Checks").unwrap();
        assert_eq!(checks_row, 3 + 10);
    }

    #[test]
    fn inaccessible_repo_is_dimmed_and_explains_why() {
        let mut app = sample_app();
        app.handle(
            Msg::Prs {
                repo: "o/b".into(),
                result: Err(ApiError::NotFound("Could not resolve".into())),
            },
            t(0),
        );
        let buffer = draw(&app, 120, 30);
        let (x, y) = find(&buffer, "o/b").unwrap();
        assert_eq!(buffer[(x, y)].fg, Color::DarkGray);

        app.apply(Action::NextRepo);
        let out = text(&draw(&app, 120, 30));
        assert!(out.contains("Cannot load o/b"));
        assert!(out.contains("Could not resolve"));
    }

    #[test]
    fn status_bar_shows_transient_errors() {
        let mut app = sample_app();
        app.report_error("cannot save config: disk full".into());
        let out = text(&draw(&app, 120, 30));
        assert!(out.contains("cannot save config: disk full"));
    }

    #[test]
    fn empty_config_shows_a_hint() {
        let app = App::new(Config::default(), "/home/me/.ghui/config.yaml".into(), t(0));
        let out = text(&draw(&app, 80, 20));
        assert!(out.contains("No repositories tracked yet."));
        assert!(out.contains("/home/me/.ghui/config.yaml"));
    }

    #[test]
    fn unloaded_and_failed_prs_are_shown_in_the_list() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        let out = text(&draw(&app, 100, 20));
        assert!(out.contains("#1 loading…"));
        app.handle(
            loaded(
                "o/a",
                vec![(1, Err("gone".into())), (2, Ok(detail(2, vec![])))],
            ),
            t(0),
        );
        let out = text(&draw(&app, 100, 20));
        assert!(out.contains("#1 gone"));
        app.apply(Action::Down);
        let out = text(&draw(&app, 100, 20));
        assert!(out.contains("No checks reported"), "{out}");
    }

    #[test]
    fn focused_check_is_highlighted() {
        let mut app = sample_app();
        app.apply(Action::FocusRight);
        app.apply(Action::Down);
        let buffer = draw(&app, 120, 30);
        let (x, y) = find(&buffer, "build").unwrap();
        assert!(
            buffer[(x, y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        assert!(text(&buffer).contains("enter logs"));
    }

    #[test]
    fn repo_state_helpers_are_consistent() {
        let app = sample_app();
        let repo: &RepoState = app.active_repo().unwrap();
        assert_eq!(repo.selected_detail().unwrap().number, 1);
    }

    #[test]
    fn elapsed_is_compact() {
        assert_eq!(elapsed(None, t(0)), "");
        assert_eq!(elapsed(Some(t(0)), t(45)), "45s");
        assert_eq!(elapsed(Some(t(0)), t(192)), "3m12s");
        assert_eq!(elapsed(Some(t(0)), t(3900)), "1h5m");
    }

    #[test]
    fn picker_marks_tracked_prs() {
        let mut app = sample_app();
        app.apply(Action::AddPr);
        let out = text(&draw(&app, 120, 30));
        assert!(out.contains("Loading open PRs…"));
        let summary = |number, title: &str, is_draft| crate::github::PrSummary {
            number,
            title: title.into(),
            author: "bob".into(),
            is_draft,
        };
        app.handle(
            Msg::OpenPrs {
                repo: "o/a".into(),
                result: Ok(vec![
                    summary(1, "tracked", false),
                    summary(7, "new one", true),
                ]),
            },
            t(0),
        );
        let out = text(&draw(&app, 120, 30));
        assert!(out.contains("Add a PR to o/a"));
        assert!(out.contains("✓ #1 tracked  bob"));
        assert!(out.contains("#7 new one  bob [draft]"));
        assert!(out.contains("enter add"));
    }

    fn open_log(app: &mut App, check_index: usize) {
        app.apply(Action::FocusRight);
        for _ in 0..check_index {
            app.apply(Action::Down);
        }
        app.apply(Action::Enter);
        app.tick(t(20));
    }

    #[test]
    fn log_window_shows_cleaned_lines_and_highlights_errors() {
        let mut app = sample_app();
        open_log(&mut app, 0); // "lint", failed
        let out = text(&draw(&app, 100, 20));
        assert!(out.contains("X lint · CI · 20s"));
        assert!(out.contains("Loading log…"));

        let raw = "\u{feff}2026-10-10T06:35:39.1Z ##[group]Run lint\n\
                   2026-10-10T06:35:39.2Z checking\n\
                   2026-10-10T06:35:39.3Z ##[error]1 warning treated as error\n";
        app.handle(
            Msg::Log {
                repo: "o/a".into(),
                job_id: job_id("lint"),
                result: Ok(Some(raw.into())),
            },
            t(20),
        );
        let buffer = draw(&app, 100, 20);
        let out = text(&buffer);
        assert!(out.contains("▸ Run lint"));
        assert!(out.contains("checking"));
        assert!(!out.contains("2026-10-10T"));
        let (x, y) = find(&buffer, "Error: 1 warning").unwrap();
        assert_eq!(buffer[(x, y)].fg, Color::Red);
        assert!(out.contains("1–3 / 3"));
        assert!(out.contains("esc close"));
    }

    #[test]
    fn running_job_without_log_says_so() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        // Running checks with a URL open in the browser; without one they
        // open the log window.
        let mut build = check("build", CheckState::Pending);
        build.url = None;
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![build])))]), t(0));
        app.now = t(20);
        open_log(&mut app, 0);
        app.handle(
            Msg::Log {
                repo: "o/a".into(),
                job_id: job_id("build"),
                result: Ok(None),
            },
            t(20),
        );
        let out = text(&draw(&app, 100, 20));
        assert!(
            out.contains("Job in progress — logs appear when it completes"),
            "{out}"
        );
    }

    #[test]
    fn non_actions_check_shows_its_summary_and_url() {
        let mut app = sample_app();
        let mut status = check("deploy", CheckState::Pass);
        status.workflow = None;
        status.kind = crate::github::CheckKind::Status {
            description: Some("Deployed to preview".into()),
        };
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![status])))]), t(20));
        open_log(&mut app, 0);
        let out = text(&draw(&app, 100, 20));
        assert!(out.contains("Deployed to preview"));
        assert!(out.contains("Details: https://ci/deploy"));
    }

    #[test]
    fn log_height_excludes_borders_and_footer() {
        assert_eq!(log_view::height(ratatui::layout::Size::new(100, 20)), 17);
    }
}
