//! Application state and its pure transitions.
//!
//! [`App::apply`] (user input), [`App::handle`] (API results) and
//! [`App::tick`] (time passing) mutate state and return [`Effect`]s for the
//! main loop to execute. None of them do I/O, so all behaviour, including
//! the poll schedule, is testable with a fake clock.

use chrono::{DateTime, TimeDelta, Utc};

use crate::config::Config;
use crate::event::Action;
use crate::github::{ApiError, PrDetail, PrResult};

/// Poll interval while the selected PR has pending checks.
pub const FAST_POLL: TimeDelta = TimeDelta::seconds(5);
/// Poll interval once the selected PR's checks have all completed.
pub const SLOW_POLL: TimeDelta = TimeDelta::seconds(60);
/// Pause after a rate limit without a reset time.
const RATE_LIMIT_BACKOFF: TimeDelta = TimeDelta::seconds(60);

/// Work for the main loop. Network effects come back as a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    FetchPrs { repo: String, numbers: Vec<u64> },
    SaveConfig(Config),
    CopyToClipboard(String),
    OpenUrl(String),
}

/// Results of network effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Prs {
        repo: String,
        result: Result<Vec<PrResult>, ApiError>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    List,
    Checks,
}

/// A tracked PR: its latest data and the error of the latest fetch, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrEntry {
    pub number: u64,
    pub detail: Option<PrDetail>,
    pub error: Option<String>,
}

impl PrEntry {
    fn new(number: u64) -> Self {
        Self {
            number,
            detail: None,
            error: None,
        }
    }
}

/// One repo tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoState {
    pub name: String,
    pub prs: Vec<PrEntry>,
    /// Repo-wide failure; the tab is dimmed while set.
    pub error: Option<ApiError>,
    pub selected_pr: usize,
    pub selected_check: usize,
    in_flight: bool,
    /// Refresh on the next tick regardless of the schedule.
    force: bool,
    /// When the last fetch completed; `None` means a fetch is due now.
    last_poll: Option<DateTime<Utc>>,
}

impl RepoState {
    fn new(name: String, prs: &[u64]) -> Self {
        Self {
            name,
            prs: prs.iter().copied().map(PrEntry::new).collect(),
            error: None,
            selected_pr: 0,
            selected_check: 0,
            in_flight: false,
            force: false,
            last_poll: None,
        }
    }

    pub fn selected(&self) -> Option<&PrEntry> {
        self.prs.get(self.selected_pr)
    }

    pub fn selected_detail(&self) -> Option<&PrDetail> {
        self.selected().and_then(|p| p.detail.as_ref())
    }

    fn has_loaded(&self) -> bool {
        self.prs.iter().any(|p| p.detail.is_some())
    }

    fn poll_interval(&self) -> TimeDelta {
        if self.error.is_some() {
            return SLOW_POLL;
        }
        match self.selected() {
            Some(pr) => match &pr.detail {
                Some(detail) if !detail.checks_running() => SLOW_POLL,
                _ => FAST_POLL,
            },
            None => SLOW_POLL,
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_pr = self.selected_pr.min(self.prs.len().saturating_sub(1));
        let checks = self.selected_detail().map_or(0, |d| d.checks.len());
        self.selected_check = self.selected_check.min(checks.saturating_sub(1));
    }
}

/// All state of the application.
#[derive(Debug)]
pub struct App {
    pub config: Config,
    /// Shown in the empty-config hint.
    pub config_path: String,
    pub repos: Vec<RepoState>,
    pub active: usize,
    pub focus: Focus,
    /// Transient error shown above the footer; cleared by the next
    /// successful fetch.
    pub status: Option<String>,
    pub rate_limited_until: Option<DateTime<Utc>>,
    /// Time of the last tick; used to render elapsed times.
    pub now: DateTime<Utc>,
    pub running: bool,
}

impl App {
    pub fn new(config: Config, config_path: String, now: DateTime<Utc>) -> Self {
        let repos = config
            .repos
            .iter()
            .map(|r| RepoState::new(r.name.clone(), &r.prs))
            .collect();
        Self {
            config,
            config_path,
            repos,
            active: 0,
            focus: Focus::default(),
            status: None,
            rate_limited_until: None,
            now,
            running: true,
        }
    }

    pub fn active_repo(&self) -> Option<&RepoState> {
        self.repos.get(self.active)
    }

    fn active_repo_mut(&mut self) -> Option<&mut RepoState> {
        self.repos.get_mut(self.active)
    }

    /// Applies a user action.
    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::ForceQuit | Action::Quit => self.running = false,
            Action::NextRepo => self.switch_repo(1),
            Action::PreviousRepo => self.switch_repo(self.repos.len().saturating_sub(1)),
            Action::FocusLeft => self.focus = Focus::List,
            Action::FocusRight => self.focus = Focus::Checks,
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
            Action::Top => self.move_selection(isize::MIN),
            Action::Bottom => self.move_selection(isize::MAX),
            Action::Refresh => {
                if let Some(repo) = self.active_repo_mut() {
                    repo.force = true;
                }
            }
            Action::Untrack => return self.untrack_selected(),
            Action::CopyUrl => {
                if let Some(url) = self.selected_pr_url() {
                    return vec![Effect::CopyToClipboard(url)];
                }
            }
            Action::OpenUrl => {
                if let Some(url) = self.focused_url() {
                    return vec![Effect::OpenUrl(url)];
                }
            }
            Action::Close | Action::PageUp | Action::PageDown | Action::Enter | Action::AddPr => {}
        }
        Vec::new()
    }

    /// Applies the result of a network effect.
    pub fn handle(&mut self, msg: Msg, now: DateTime<Utc>) {
        match msg {
            Msg::Prs { repo, result } => self.handle_prs(&repo, result, now),
        }
    }

    /// Advances the clock and returns the polls that are due.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<Effect> {
        self.now = now;
        if let Some(until) = self.rate_limited_until {
            if now < until {
                return Vec::new();
            }
            self.rate_limited_until = None;
            self.status = None;
        }
        let mut effects = Vec::new();
        if let Some(repo) = self.repos.get_mut(self.active) {
            // The interval depends on the selected PR, so it is evaluated
            // here rather than when the previous fetch completed.
            let due = repo.force
                || repo
                    .last_poll
                    .is_none_or(|last| now >= last + repo.poll_interval());
            if due && !repo.in_flight {
                repo.in_flight = true;
                repo.force = false;
                effects.push(Effect::FetchPrs {
                    repo: repo.name.clone(),
                    numbers: repo.prs.iter().map(|p| p.number).collect(),
                });
            }
        }
        effects
    }

    /// Records a failure to save the config.
    pub fn report_error(&mut self, message: String) {
        self.status = Some(message);
    }

    fn handle_prs(
        &mut self,
        name: &str,
        result: Result<Vec<PrResult>, ApiError>,
        now: DateTime<Utc>,
    ) {
        let Some(repo) = self.repos.iter_mut().find(|r| r.name == name) else {
            return;
        };
        repo.in_flight = false;
        match result {
            Ok(results) => {
                repo.error = None;
                for (number, result) in results {
                    // PRs untracked while the request was in flight are skipped.
                    let Some(entry) = repo.prs.iter_mut().find(|p| p.number == number) else {
                        continue;
                    };
                    match result {
                        Ok(detail) => {
                            entry.detail = Some(detail);
                            entry.error = None;
                        }
                        Err(e) => entry.error = Some(e),
                    }
                }
                repo.clamp_selection();
                self.status = None;
            }
            Err(ApiError::RateLimited { reset }) => {
                let until = reset
                    .filter(|r| *r > now)
                    .unwrap_or(now + RATE_LIMIT_BACKOFF);
                self.rate_limited_until = Some(until);
                self.status = Some(format!(
                    "Rate limited — polling paused until {}",
                    until.format("%H:%M:%S UTC")
                ));
            }
            Err(e @ (ApiError::NotFound(_) | ApiError::Forbidden(_))) => repo.error = Some(e),
            Err(e) if !repo.has_loaded() => repo.error = Some(e),
            // Keep showing the last good data.
            Err(e) => self.status = Some(format!("{}: refresh failed: {e}", repo.name)),
        }
        repo.last_poll = Some(now);
    }

    fn switch_repo(&mut self, step: usize) {
        if !self.repos.is_empty() {
            self.active = (self.active + step) % self.repos.len();
        }
    }

    /// Moves the selection in the focused pane by `delta`, clamped.
    fn move_selection(&mut self, delta: isize) {
        let focus = self.focus;
        let Some(repo) = self.active_repo_mut() else {
            return;
        };
        match focus {
            Focus::List => {
                let before = repo.selected_pr;
                repo.selected_pr = step(repo.selected_pr, delta, repo.prs.len());
                if repo.selected_pr != before {
                    repo.selected_check = 0;
                }
            }
            Focus::Checks => {
                let len = repo.selected_detail().map_or(0, |d| d.checks.len());
                repo.selected_check = step(repo.selected_check, delta, len);
            }
        }
    }

    fn untrack_selected(&mut self) -> Vec<Effect> {
        let Some(repo) = self.repos.get_mut(self.active) else {
            return Vec::new();
        };
        let Some(number) = repo.selected().map(|p| p.number) else {
            return Vec::new();
        };
        repo.prs.retain(|p| p.number != number);
        repo.selected_check = 0;
        repo.clamp_selection();
        self.config.untrack(&repo.name, number);
        vec![Effect::SaveConfig(self.config.clone())]
    }

    fn selected_pr_url(&self) -> Option<String> {
        Some(self.active_repo()?.selected_detail()?.url.clone())
    }

    /// The selected check's URL when the checks pane is focused, else the
    /// PR's.
    fn focused_url(&self) -> Option<String> {
        let detail = self.active_repo()?.selected_detail()?;
        if self.focus == Focus::Checks {
            let check = detail.checks.get(self.active_repo()?.selected_check);
            if let Some(url) = check.and_then(|c| c.url.clone()) {
                return Some(url);
            }
        }
        Some(detail.url.clone())
    }
}

/// `index + delta`, clamped to `0..len`.
fn step(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    index.saturating_add_signed(delta).min(len - 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::RepoConfig;
    use crate::github::{Check, CheckKind, CheckState, PrState};

    pub fn t(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    pub fn config(repos: &[(&str, &[u64])]) -> Config {
        Config {
            repos: repos
                .iter()
                .map(|(name, prs)| RepoConfig {
                    name: name.to_string(),
                    prs: prs.to_vec(),
                })
                .collect(),
        }
    }

    pub fn check(name: &str, state: CheckState) -> Check {
        Check {
            name: name.into(),
            workflow: Some("CI".into()),
            state,
            started_at: Some(t(-30)),
            completed_at: (state != CheckState::Pending).then(|| t(-10)),
            url: Some(format!("https://ci/{name}")),
            kind: CheckKind::ActionsJob { job_id: 7 },
        }
    }

    pub fn detail(number: u64, checks: Vec<Check>) -> PrDetail {
        PrDetail {
            number,
            title: format!("PR {number}"),
            body: "Body".into(),
            url: format!("https://github.com/o/r/pull/{number}"),
            state: PrState::Open,
            author: "me".into(),
            head: "feature".into(),
            base: "main".into(),
            checks,
        }
    }

    pub fn loaded(repo: &str, results: Vec<PrResult>) -> Msg {
        Msg::Prs {
            repo: repo.into(),
            result: Ok(results),
        }
    }

    fn fetch(repo: &str, numbers: &[u64]) -> Effect {
        Effect::FetchPrs {
            repo: repo.into(),
            numbers: numbers.to_vec(),
        }
    }

    #[test]
    fn first_tick_fetches_every_pr_of_the_active_repo() {
        let mut app = App::new(
            config(&[("o/a", &[1, 2]), ("o/b", &[3])]),
            "cfg".into(),
            t(0),
        );
        assert_eq!(app.tick(t(0)), vec![fetch("o/a", &[1, 2])]);
        // Nothing more while the request is in flight.
        assert!(app.tick(t(10)).is_empty());
    }

    #[test]
    fn polls_fast_while_checks_run_and_slow_after() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        app.handle(loaded("o/a", vec![(1, Ok(running))]), t(1));
        assert!(app.tick(t(5)).is_empty());
        assert_eq!(app.tick(t(6)), vec![fetch("o/a", &[1])]);

        let done = detail(1, vec![check("build", CheckState::Pass)]);
        app.handle(loaded("o/a", vec![(1, Ok(done))]), t(7));
        assert!(app.tick(t(66)).is_empty());
        assert_eq!(app.tick(t(67)), vec![fetch("o/a", &[1])]);
    }

    #[test]
    fn refresh_forces_a_poll() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(1));
        assert!(app.tick(t(2)).is_empty());
        app.apply(Action::Refresh);
        assert_eq!(app.tick(t(2)), vec![fetch("o/a", &[1])]);
    }

    #[test]
    fn switching_tabs_polls_the_new_repo() {
        let mut app = App::new(config(&[("o/a", &[1]), ("o/b", &[2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.apply(Action::NextRepo);
        assert_eq!(app.active, 1);
        assert_eq!(app.tick(t(1)), vec![fetch("o/b", &[2])]);
        app.apply(Action::NextRepo);
        assert_eq!(app.active, 0);
        app.apply(Action::PreviousRepo);
        assert_eq!(app.active, 1);
    }

    #[test]
    fn inaccessible_repo_is_marked_and_retried_slowly() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        let err = ApiError::NotFound("nope".into());
        app.handle(
            Msg::Prs {
                repo: "o/a".into(),
                result: Err(err.clone()),
            },
            t(1),
        );
        assert_eq!(app.repos[0].error, Some(err));
        assert!(app.tick(t(60)).is_empty());
        assert_eq!(app.tick(t(61)).len(), 1);
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(62));
        assert_eq!(app.repos[0].error, None);
    }

    #[test]
    fn network_error_after_a_load_keeps_data_and_sets_status() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(1));
        app.apply(Action::Refresh);
        app.tick(t(2));
        app.handle(
            Msg::Prs {
                repo: "o/a".into(),
                result: Err(ApiError::Network("down".into())),
            },
            t(3),
        );
        assert_eq!(app.repos[0].error, None);
        assert!(app.repos[0].prs[0].detail.is_some());
        assert!(app.status.as_deref().unwrap().contains("down"));
    }

    #[test]
    fn rate_limit_pauses_polling_until_reset() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(
            Msg::Prs {
                repo: "o/a".into(),
                result: Err(ApiError::RateLimited {
                    reset: Some(t(100)),
                }),
            },
            t(1),
        );
        assert!(app.status.as_deref().unwrap().contains("Rate limited"));
        assert!(app.tick(t(99)).is_empty());
        assert_eq!(app.tick(t(100)).len(), 1);
        assert_eq!(app.status, None);
    }

    #[test]
    fn per_pr_errors_are_kept_on_the_entry() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![]))), (2, Err("missing".into()))],
            ),
            t(1),
        );
        assert!(app.repos[0].prs[0].detail.is_some());
        assert_eq!(app.repos[0].prs[1].error.as_deref(), Some("missing"));
    }

    #[test]
    fn selection_moves_in_the_focused_pane_and_clamps() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        let checks = vec![check("a", CheckState::Fail), check("b", CheckState::Pass)];
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, checks))), (2, Ok(detail(2, vec![])))],
            ),
            t(1),
        );
        app.apply(Action::FocusRight);
        app.apply(Action::Down);
        app.apply(Action::Down);
        assert_eq!(app.repos[0].selected_check, 1);
        app.apply(Action::Top);
        assert_eq!(app.repos[0].selected_check, 0);

        app.apply(Action::FocusLeft);
        app.apply(Action::Up);
        assert_eq!(app.repos[0].selected_pr, 0);
        app.apply(Action::Bottom);
        assert_eq!(app.repos[0].selected_pr, 1);
    }

    #[test]
    fn selecting_a_running_pr_switches_to_the_fast_poll() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        let done = vec![check("a", CheckState::Pass), check("b", CheckState::Pass)];
        let running = vec![check("c", CheckState::Pending)];
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, done))), (2, Ok(detail(2, running)))],
            ),
            t(1),
        );
        assert!(app.tick(t(6)).is_empty());
        app.repos[0].selected_check = 1;
        app.apply(Action::Down);
        assert_eq!(app.repos[0].selected_check, 0);
        assert_eq!(app.tick(t(6)).len(), 1);
    }

    #[test]
    fn untrack_removes_the_pr_and_saves() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.apply(Action::Down);
        let effects = app.apply(Action::Untrack);
        assert_eq!(effects, vec![Effect::SaveConfig(config(&[("o/a", &[1])]))]);
        assert_eq!(app.repos[0].prs.len(), 1);
        assert_eq!(app.repos[0].selected_pr, 0);
    }

    #[test]
    fn copy_and_open_use_the_pr_or_focused_check_url() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        assert!(app.apply(Action::CopyUrl).is_empty());
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![check("b", CheckState::Pass)])))],
            ),
            t(1),
        );
        let pr_url = "https://github.com/o/r/pull/1".to_string();
        assert_eq!(
            app.apply(Action::CopyUrl),
            vec![Effect::CopyToClipboard(pr_url.clone())]
        );
        assert_eq!(app.apply(Action::OpenUrl), vec![Effect::OpenUrl(pr_url)]);
        app.apply(Action::FocusRight);
        assert_eq!(
            app.apply(Action::OpenUrl),
            vec![Effect::OpenUrl("https://ci/b".into())]
        );
    }

    #[test]
    fn quit_stops_the_app() {
        let mut app = App::new(Config::default(), "cfg".into(), t(0));
        assert!(app.tick(t(0)).is_empty());
        app.apply(Action::Quit);
        assert!(!app.running);
    }
}
