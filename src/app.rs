//! Application state and its pure transitions.
//!
//! [`App::apply`] (user input), [`App::handle`] (API results) and
//! [`App::tick`] (time passing) mutate state and return [`Effect`]s for the
//! main loop to execute. None of them do I/O, so all behaviour, including
//! the poll schedule, is testable with a fake clock.

use chrono::{DateTime, TimeDelta, Utc};

use crate::api_log::{ApiCall, ApiLog};
use crate::check_tree::{self, Folds, Node, Row};
use crate::config::Config;
use crate::event::Action;
use crate::github::{ApiError, CheckKind, CheckState, OpenPrs, PrDetail, PrResult, PrSummary};
use crate::log_view::LogView;
use crate::scroll::Scroll;

/// Poll interval of a repo's PRs with pending checks; see spec 0008.
pub const CHECK_POLL: TimeDelta = TimeDelta::seconds(30);
/// Retry interval of the active repo when it failed to load.
pub const RETRY_POLL: TimeDelta = TimeDelta::seconds(60);
/// Pause after a rate limit without a reset time.
const RATE_LIMIT_BACKOFF: TimeDelta = TimeDelta::seconds(60);

/// Work for the main loop. Network effects come back as a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    FetchPrs {
        repo: String,
        numbers: Vec<u64>,
        /// The PR to fetch job steps for; see spec 0007.
        steps: Option<u64>,
    },
    FetchOpenPrs {
        repo: String,
    },
    FetchLog {
        repo: String,
        job_id: u64,
    },
    SaveConfig(Config),
    CopyToClipboard(String),
    OpenUrl(String),
    /// A desktop notification; see spec 0004.
    Notify {
        title: String,
        body: String,
    },
}

/// Results of network effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Prs {
        repo: String,
        result: Result<Vec<PrResult>, ApiError>,
    },
    OpenPrs {
        repo: String,
        result: Result<OpenPrs, ApiError>,
    },
    Log {
        repo: String,
        job_id: u64,
        result: Result<Option<String>, ApiError>,
    },
    /// Sending a desktop notification failed.
    NotifyFailed(String),
    /// A GitHub API request completed; sent before its result.
    ApiCall(ApiCall),
}

/// Rows moved by `PgUp` / `PgDn` in lists.
const PAGE: isize = 10;

/// A window drawn over the main view that takes all input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    Picker(Picker),
    Log(Box<LogView>),
    /// The API call log ([`App::api_log`]); see spec 0009.
    ApiLog(Scroll),
}

/// The add-PR picker: open PRs of `repo`; see spec 0008.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub repo: String,
    /// `None` while loading.
    pub items: Option<Result<OpenPrs, String>>,
    /// Show the user's PRs rather than all.
    pub mine: bool,
    /// Typed digits; only PRs whose number starts with them are shown.
    pub filter: String,
    /// Index into [`Picker::visible`].
    pub selected: usize,
}

impl Picker {
    fn new(repo: String) -> Self {
        Self {
            repo,
            items: None,
            mine: true,
            filter: String::new(),
            selected: 0,
        }
    }

    /// The rows shown: the current listing, filtered by number.
    pub fn visible(&self) -> Vec<&PrSummary> {
        let Some(Ok(items)) = &self.items else {
            return Vec::new();
        };
        let list = if self.mine { &items.mine } else { &items.all };
        list.iter()
            .filter(|p| p.number.to_string().starts_with(&self.filter))
            .collect()
    }

    /// The PR `Enter` tracks: the selected row, or the typed number when
    /// no row matches.
    fn target(&self) -> Option<u64> {
        let visible = self.visible();
        if visible.is_empty() {
            return self.filter.parse().ok();
        }
        visible.get(self.selected).map(|p| p.number)
    }
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
    /// Row of the checks tree; see [`RepoState::check_rows`].
    pub selected_check: usize,
    /// Manual expand/collapse of runs and jobs in the checks tree.
    pub folds: Folds,
    in_flight: bool,
    /// Refresh on the next tick regardless of the schedule, even when the
    /// tab is inactive or failed to load.
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
            folds: Folds::new(),
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

    /// The visible rows of the selected PR's checks tree.
    pub fn check_rows(&self) -> Vec<Row<'_>> {
        self.selected_detail()
            .map(|d| check_tree::rows(d, &self.folds))
            .unwrap_or_default()
    }

    fn selected_row(&self) -> Option<Row<'_>> {
        self.check_rows().into_iter().nth(self.selected_check)
    }

    fn has_loaded(&self) -> bool {
        self.prs.iter().any(|p| p.detail.is_some())
    }

    /// The PRs whose checks are still pending.
    fn pending(&self) -> Vec<u64> {
        self.prs
            .iter()
            .filter(|p| p.detail.as_ref().is_some_and(PrDetail::checks_running))
            .map(|p| p.number)
            .collect()
    }

    /// Whether every PR must be fetched rather than the pending ones:
    /// first load, retries and manual refreshes.
    fn full_fetch(&self) -> bool {
        self.force || self.last_poll.is_none() || self.error.is_some()
    }

    /// Whether a fetch is due at `now`. After the first load, a repo is
    /// polled every [`CHECK_POLL`] while it has pending checks, active or
    /// not; a failed one is retried while active. See spec 0008.
    fn poll_due(&self, active: bool, now: DateTime<Utc>) -> bool {
        if self.in_flight {
            return false;
        }
        let Some(last) = self.last_poll.filter(|_| !self.force) else {
            return true;
        };
        let interval = if self.error.is_some() {
            if !active {
                return false;
            }
            RETRY_POLL
        } else if self.pending().is_empty() {
            return false;
        } else {
            CHECK_POLL
        };
        now >= last + interval
    }

    /// Drops the PRs `numbers`. The selected PR stays selected; if it is
    /// dropped the selection stays at the same row, on the first check.
    fn remove_prs(&mut self, numbers: &[u64]) {
        let selected = self.selected().map(|p| p.number);
        self.prs.retain(|p| !numbers.contains(&p.number));
        match selected.and_then(|n| self.prs.iter().position(|p| p.number == n)) {
            Some(i) => self.selected_pr = i,
            None => self.selected_check = 0,
        }
        self.clamp_selection();
    }

    /// Expands or collapses the selected run or job; whether it changed.
    fn set_fold(&mut self, expanded: bool) -> bool {
        let Some((fold, current)) = self.selected_row().and_then(|r| r.fold) else {
            return false;
        };
        if current == expanded {
            return false;
        }
        self.folds.insert(fold, expanded);
        true
    }

    /// `←` in the checks tree: collapses the selected row, or else selects
    /// its parent. `false` when neither applies (a top-level row).
    fn fold_left(&mut self) -> bool {
        if self.set_fold(false) {
            return true;
        }
        let rows = self.check_rows();
        let Some(depth) = rows.get(self.selected_check).map(|r| r.depth) else {
            return false;
        };
        let parent = rows[..self.selected_check]
            .iter()
            .rposition(|r| r.depth < depth);
        match parent {
            Some(parent) => {
                self.selected_check = parent;
                true
            }
            None => false,
        }
    }

    /// `Space` in the checks tree: toggles the selected run or job; on a
    /// step, collapses its job and selects it.
    fn toggle_fold(&mut self) {
        let selected = self
            .selected_row()
            .map(|r| (r.fold, matches!(r.node, Node::Step { .. })));
        match selected {
            Some((Some((fold, expanded)), _)) => {
                self.folds.insert(fold, !expanded);
            }
            Some((None, true)) => {
                self.fold_left();
                self.set_fold(false);
            }
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_pr = self.selected_pr.min(self.prs.len().saturating_sub(1));
        let rows = self.check_rows().len();
        self.selected_check = self.selected_check.min(rows.saturating_sub(1));
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
    pub modal: Option<Modal>,
    /// Visible lines in the log window; set by the main loop from the
    /// terminal size so scrolling can clamp.
    pub log_height: usize,
    /// Transient error shown above the footer; cleared by the next
    /// successful fetch.
    pub status: Option<String>,
    pub rate_limited_until: Option<DateTime<Utc>>,
    /// Time of the last tick; used to render elapsed times.
    pub now: DateTime<Utc>,
    pub running: bool,
    /// A manual refresh (`r`) has PR fetches pending; shown in the tab bar.
    pub refreshing: bool,
    /// Every GitHub API call of the session; see spec 0009.
    pub api_log: ApiLog,
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
            modal: None,
            log_height: 20,
            status: None,
            rate_limited_until: None,
            now,
            running: true,
            refreshing: false,
            api_log: ApiLog::default(),
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
        if action == Action::ForceQuit {
            self.running = false;
            return Vec::new();
        }
        if let Some(modal) = self.modal.take() {
            return self.apply_modal(modal, action);
        }
        match action {
            Action::ForceQuit | Action::Quit => self.running = false,
            Action::NextRepo => self.switch_repo(1),
            Action::PreviousRepo => self.switch_repo(self.repos.len().saturating_sub(1)),
            Action::FocusLeft => self.focus_left(),
            Action::FocusRight => self.focus_right(),
            Action::Toggle => {
                if self.focus == Focus::Checks
                    && let Some(repo) = self.active_repo_mut()
                {
                    repo.toggle_fold();
                }
            }
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
            Action::Top => self.move_selection(isize::MIN),
            Action::Bottom => self.move_selection(isize::MAX),
            Action::Refresh => self.refresh_all(),
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
            Action::PageUp => self.move_selection(-PAGE),
            Action::PageDown => self.move_selection(PAGE),
            Action::AddPr => return self.open_picker(),
            Action::ApiLog => self.modal = Some(Modal::ApiLog(Scroll::default())),
            Action::Enter => match self.focus {
                Focus::List => {
                    if let Some(url) = self.selected_pr_url() {
                        return vec![Effect::OpenUrl(url)];
                    }
                }
                Focus::Checks => return self.enter_check(),
            },
            // Picker-only keys.
            Action::Close | Action::ToggleMine | Action::Digit(_) | Action::Backspace => {}
        }
        Vec::new()
    }

    /// Input while a modal is open. The modal has been taken out of `self`
    /// and is put back unless the action closes it.
    fn apply_modal(&mut self, modal: Modal, action: Action) -> Vec<Effect> {
        if matches!(action, Action::Quit | Action::Close) {
            return Vec::new();
        }
        match modal {
            Modal::Picker(mut picker) => {
                let delta = match action {
                    Action::Up => -1,
                    Action::Down => 1,
                    Action::PageUp => -PAGE,
                    Action::PageDown => PAGE,
                    Action::Top => isize::MIN,
                    Action::Bottom => isize::MAX,
                    Action::Enter => return self.pick(picker),
                    Action::ToggleMine => {
                        picker.mine = !picker.mine;
                        picker.selected = 0;
                        0
                    }
                    Action::Digit(d) => {
                        picker.filter.push(char::from(b'0' + d));
                        picker.selected = 0;
                        0
                    }
                    Action::Backspace => {
                        picker.filter.pop();
                        picker.selected = 0;
                        0
                    }
                    _ => 0,
                };
                let len = picker.visible().len();
                picker.selected = step(picker.selected, delta, len);
                self.modal = Some(Modal::Picker(picker));
            }
            Modal::Log(mut log) => {
                let height = self.log_height.max(1);
                let page = isize::try_from(height).unwrap_or(isize::MAX);
                let mut effects = Vec::new();
                match action {
                    Action::Up => log.scroll_by(-1, height),
                    Action::Down => log.scroll_by(1, height),
                    Action::PageUp => log.scroll_by(-page, height),
                    Action::PageDown => log.scroll_by(page, height),
                    Action::Top => log.scroll_by(isize::MIN, height),
                    Action::Bottom => log.scroll_by(isize::MAX, height),
                    Action::Refresh => log.refresh(),
                    Action::OpenUrl | Action::CopyUrl => {
                        if let Some(url) = log.check.url.clone() {
                            effects.push(if action == Action::OpenUrl {
                                Effect::OpenUrl(url)
                            } else {
                                Effect::CopyToClipboard(url)
                            });
                        }
                    }
                    _ => {}
                }
                self.modal = Some(Modal::Log(log));
                return effects;
            }
            Modal::ApiLog(mut scroll) => {
                let height = self.log_height.max(1);
                let page = isize::try_from(height).unwrap_or(isize::MAX);
                let delta = match action {
                    Action::Up => -1,
                    Action::Down => 1,
                    Action::PageUp => -page,
                    Action::PageDown => page,
                    Action::Top => isize::MIN,
                    Action::Bottom => isize::MAX,
                    _ => 0,
                };
                if delta != 0 {
                    scroll.scroll_by(delta, self.api_log.len(), height);
                }
                self.modal = Some(Modal::ApiLog(scroll));
            }
        }
        Vec::new()
    }

    /// `←`: in the checks tree, collapses the selected row or moves to its
    /// parent; from the top level, focuses the PR list.
    fn focus_left(&mut self) {
        if self.focus == Focus::Checks
            && let Some(repo) = self.active_repo_mut()
            && repo.fold_left()
        {
            return;
        }
        self.focus = Focus::List;
    }

    /// `→`: focuses the checks, or expands the selected row there.
    fn focus_right(&mut self) {
        match self.focus {
            Focus::List => self.focus = Focus::Checks,
            Focus::Checks => {
                if let Some(repo) = self.active_repo_mut() {
                    repo.set_fold(true);
                }
            }
        }
    }

    /// `Enter` on the selected row of the checks tree: a run toggles; a
    /// running job (or its step) opens in the browser, since the API has no
    /// live log; any other job or check opens the log window.
    fn enter_check(&mut self) -> Vec<Effect> {
        let Some(repo) = self.active_repo() else {
            return Vec::new();
        };
        let Some(detail) = repo.selected_detail() else {
            return Vec::new();
        };
        let Some(row) = repo.selected_row() else {
            return Vec::new();
        };
        let Some(check) = row.check() else {
            if let Some(repo) = self.active_repo_mut() {
                repo.toggle_fold();
            }
            return Vec::new();
        };
        if check.is_running()
            && let Some(url) = row.url()
        {
            return vec![Effect::OpenUrl(url)];
        }
        let view = LogView::new(repo.name.clone(), detail.number, check.clone());
        self.modal = Some(Modal::Log(Box::new(view)));
        Vec::new()
    }

    fn open_picker(&mut self) -> Vec<Effect> {
        let Some(repo) = self.active_repo() else {
            self.status = Some(format!("Add a repo to {} first", self.config_path));
            return Vec::new();
        };
        let repo = repo.name.clone();
        self.modal = Some(Modal::Picker(Picker::new(repo.clone())));
        vec![Effect::FetchOpenPrs { repo }]
    }

    /// Tracks the PR under the cursor, or the typed number when no row
    /// matches, closes the picker and selects it. Already tracked PRs leave
    /// the picker open.
    fn pick(&mut self, picker: Picker) -> Vec<Effect> {
        let number = picker.target();
        let Some(repo) = self.repos.iter_mut().find(|r| r.name == picker.repo) else {
            return Vec::new();
        };
        let Some(number) = number.filter(|n| !repo.prs.iter().any(|p| p.number == *n)) else {
            self.modal = Some(Modal::Picker(picker));
            return Vec::new();
        };
        repo.prs.push(PrEntry::new(number));
        repo.selected_pr = repo.prs.len() - 1;
        repo.selected_check = 0;
        repo.force = true;
        self.focus = Focus::List;
        self.config.track(&picker.repo, number);
        vec![Effect::SaveConfig(self.config.clone())]
    }

    /// Whether `number` is tracked in `repo`; used to mark picker rows.
    pub fn is_tracked(&self, repo: &str, number: u64) -> bool {
        self.repos
            .iter()
            .any(|r| r.name == repo && r.prs.iter().any(|p| p.number == number))
    }

    /// Applies the result of a network effect.
    pub fn handle(&mut self, msg: Msg, now: DateTime<Utc>) -> Vec<Effect> {
        match msg {
            Msg::Prs { repo, result } => return self.handle_prs(&repo, result, now),
            Msg::OpenPrs { repo, result } => {
                if let Err(ApiError::RateLimited { reset }) = &result {
                    self.rate_limited(*reset, now);
                }
                // A picker that already shows PRs is not reset under the
                // cursor.
                if let Some(Modal::Picker(picker)) = &mut self.modal
                    && picker.repo == repo
                    && !matches!(picker.items, Some(Ok(_)))
                {
                    picker.items = Some(result.map_err(|e| e.to_string()));
                    picker.selected = 0;
                }
            }
            Msg::NotifyFailed(message) => self.report_error(message),
            Msg::ApiCall(call) => self.api_log.push(call),
            Msg::Log {
                repo,
                job_id,
                result,
            } => {
                if let Err(ApiError::RateLimited { reset }) = &result {
                    self.rate_limited(*reset, now);
                }
                if let Some(Modal::Log(log)) = &mut self.modal
                    && log.repo == repo
                    && log.job_id() == Some(job_id)
                {
                    log.finish_fetch(result.map_err(|e| e.to_string()), now);
                }
            }
        }
        Vec::new()
    }

    /// Advances the clock and returns the polls that are due.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<Effect> {
        self.now = now;
        if !self.repos.iter().any(|r| r.force || r.in_flight) {
            self.refreshing = false;
        }
        if let Some(until) = self.rate_limited_until {
            if now < until {
                return Vec::new();
            }
            self.rate_limited_until = None;
            self.status = None;
        }
        let mut effects = Vec::new();
        for (i, repo) in self.repos.iter_mut().enumerate() {
            let active = i == self.active;
            if repo.poll_due(active, now) {
                let numbers = if repo.full_fetch() {
                    repo.prs.iter().map(|p| p.number).collect()
                } else {
                    repo.pending()
                };
                repo.in_flight = true;
                repo.force = false;
                // Steps are costly, so only the PR on screen gets them.
                let steps = repo
                    .selected()
                    .map(|p| p.number)
                    .filter(|n| active && numbers.contains(n));
                effects.push(Effect::FetchPrs {
                    repo: repo.name.clone(),
                    numbers,
                    steps,
                });
            }
        }
        effects.extend(self.tick_log(now));
        effects
    }

    fn rate_limited(&mut self, reset: Option<DateTime<Utc>>, now: DateTime<Utc>) {
        let until = reset
            .filter(|r| *r > now)
            .unwrap_or(now + RATE_LIMIT_BACKOFF);
        self.rate_limited_until = Some(until);
        self.status = Some(format!(
            "Rate limited — polling paused until {}",
            until.format("%H:%M:%S UTC")
        ));
    }

    /// Keeps the open log window's check in sync with the latest PR data
    /// and schedules its log fetch.
    fn tick_log(&mut self, now: DateTime<Utc>) -> Option<Effect> {
        let Some(Modal::Log(log)) = &mut self.modal else {
            return None;
        };
        let job_id = log.job_id()?;
        let latest = self
            .repos
            .iter()
            .find(|r| r.name == log.repo)
            .and_then(|r| r.prs.iter().find(|p| p.number == log.pr))
            .and_then(|p| p.detail.as_ref())
            .and_then(|d| {
                d.checks
                    .iter()
                    .find(|c| c.kind == CheckKind::ActionsJob { job_id })
            });
        if let Some(check) = latest {
            log.check = check.clone();
        }
        if !log.poll_due(now) {
            return None;
        }
        log.start_fetch();
        Some(Effect::FetchLog {
            repo: log.repo.clone(),
            job_id,
        })
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
    ) -> Vec<Effect> {
        let Some(repo_index) = self.repos.iter().position(|r| r.name == name) else {
            return Vec::new();
        };
        let repo = &mut self.repos[repo_index];
        repo.in_flight = false;
        let mut effects = Vec::new();
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
                            effects.extend(checks_finished(name, entry.detail.as_ref(), &detail));
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
                repo.last_poll = Some(now);
                // Retry everything once the limit resets.
                repo.force = true;
                self.rate_limited(reset, now);
                return effects;
            }
            Err(e @ (ApiError::NotFound(_) | ApiError::Forbidden(_))) => repo.error = Some(e),
            Err(e) if !repo.has_loaded() => repo.error = Some(e),
            // Keep showing the last good data.
            Err(e) => self.status = Some(format!("{}: refresh failed: {e}", repo.name)),
        }
        self.repos[repo_index].last_poll = Some(now);
        effects
    }

    /// `r`: re-fetches every PR of every tab; see specs 0006 and 0008.
    fn refresh_all(&mut self) {
        self.refreshing = !self.repos.is_empty();
        for repo in &mut self.repos {
            repo.force = true;
        }
    }

    fn switch_repo(&mut self, step: usize) {
        if !self.repos.is_empty() {
            self.active = (self.active + step) % self.repos.len();
            // Background polls may have left the tab stale.
            self.repos[self.active].force = true;
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
                    // Only the selected PR is fetched with its job steps.
                    repo.force = true;
                }
            }
            Focus::Checks => {
                let len = repo.check_rows().len();
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
        repo.remove_prs(&[number]);
        self.config.untrack(&repo.name, number);
        vec![Effect::SaveConfig(self.config.clone())]
    }

    fn selected_pr_url(&self) -> Option<String> {
        Some(self.active_repo()?.selected_detail()?.url.clone())
    }

    /// The selected row's URL when the checks pane is focused, else the
    /// PR's.
    fn focused_url(&self) -> Option<String> {
        let repo = self.active_repo()?;
        let detail = repo.selected_detail()?;
        if self.focus == Focus::Checks
            && let Some(url) = repo.selected_row().and_then(|r| r.url())
        {
            return Some(url);
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

/// The notification for a PR whose checks were running at the previous
/// poll (`old`) and have all completed now (`new`); see spec 0004.
fn checks_finished(repo: &str, old: Option<&PrDetail>, new: &PrDetail) -> Option<Effect> {
    let was_running = old.is_some_and(PrDetail::checks_running);
    if !was_running || new.checks_running() || new.checks.is_empty() {
        return None;
    }
    let failed = new
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Fail)
        .count();
    let pr = format!("{repo} #{} {}", new.number, new.title);
    Some(if failed == 0 {
        Effect::Notify {
            title: "Checks passed".into(),
            body: pr,
        }
    } else {
        Effect::Notify {
            title: "Checks failed".into(),
            body: format!("{pr}\n{failed} of {} checks failed", new.checks.len()),
        }
    })
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
            kind: CheckKind::ActionsJob {
                job_id: job_id(name),
            },
            run_id: None,
            steps: Vec::new(),
        }
    }

    /// A distinct fake job id per check name.
    pub fn job_id(name: &str) -> u64 {
        name.bytes().map(u64::from).sum()
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

    /// A poll of the active tab, with steps for its selected (first) PR.
    fn fetch(repo: &str, numbers: &[u64]) -> Effect {
        Effect::FetchPrs {
            repo: repo.into(),
            numbers: numbers.to_vec(),
            steps: numbers.first().copied(),
        }
    }

    /// A poll with steps for PR `steps`.
    fn fetch_with(repo: &str, numbers: &[u64], steps: Option<u64>) -> Effect {
        Effect::FetchPrs {
            repo: repo.into(),
            numbers: numbers.to_vec(),
            steps,
        }
    }

    /// A background poll of an inactive tab, without steps.
    fn fetch_bg(repo: &str, numbers: &[u64]) -> Effect {
        fetch_with(repo, numbers, None)
    }

    #[test]
    fn first_tick_fetches_every_repo() {
        let mut app = App::new(
            config(&[("o/a", &[1, 2]), ("o/b", &[3])]),
            "cfg".into(),
            t(0),
        );
        assert_eq!(
            app.tick(t(0)),
            vec![fetch("o/a", &[1, 2]), fetch_bg("o/b", &[3])]
        );
        // Nothing more while the requests are in flight.
        assert!(app.tick(t(10)).is_empty());
    }

    #[test]
    fn only_pending_prs_are_polled_every_30_s_until_done() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        let done = detail(2, vec![check("build", CheckState::Pass)]);
        app.handle(loaded("o/a", vec![(1, Ok(running)), (2, Ok(done))]), t(1));
        assert!(app.tick(t(30)).is_empty());
        assert_eq!(app.tick(t(31)), vec![fetch("o/a", &[1])]);

        let finished = detail(1, vec![check("build", CheckState::Pass)]);
        app.handle(loaded("o/a", vec![(1, Ok(finished))]), t(32));
        assert!(app.tick(t(10_000)).is_empty());
    }

    #[test]
    fn pending_poll_fetches_steps_only_for_a_selected_pending_pr() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        let done = detail(2, vec![check("build", CheckState::Pass)]);
        app.handle(
            loaded("o/a", vec![(1, Ok(running.clone())), (2, Ok(done))]),
            t(1),
        );
        app.apply(Action::Down);
        // Selecting fetches the whole tab, with the new selection's steps.
        assert_eq!(app.tick(t(2)), vec![fetch_with("o/a", &[1, 2], Some(2))]);
        let done = detail(2, vec![check("build", CheckState::Pass)]);
        app.handle(loaded("o/a", vec![(1, Ok(running)), (2, Ok(done))]), t(3));
        assert_eq!(app.tick(t(33)), vec![fetch_with("o/a", &[1], None)]);
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
        app.handle(loaded("o/b", vec![(2, Ok(detail(2, vec![])))]), t(1));
        app.apply(Action::NextRepo);
        assert_eq!(app.active, 1);
        assert_eq!(app.tick(t(1)), vec![fetch("o/b", &[2])]);
        app.apply(Action::NextRepo);
        assert_eq!(app.active, 0);
        app.apply(Action::PreviousRepo);
        assert_eq!(app.active, 1);
    }

    #[test]
    fn inactive_repo_is_polled_while_checks_run() {
        let mut app = App::new(config(&[("o/a", &[1]), ("o/b", &[2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        let running = detail(2, vec![check("build", CheckState::Pending)]);
        app.handle(loaded("o/b", vec![(2, Ok(running))]), t(0));
        assert!(app.tick(t(29)).is_empty());
        assert_eq!(app.tick(t(30)), vec![fetch_bg("o/b", &[2])]);

        let done = detail(2, vec![check("build", CheckState::Pass)]);
        let effects = app.handle(loaded("o/b", vec![(2, Ok(done))]), t(30));
        assert_eq!(effects, vec![notify("Checks passed", "o/b #2 PR 2")]);
        assert!(app.tick(t(10_000)).is_empty());
    }

    #[test]
    fn refresh_refetches_every_tab_and_keeps_closed_prs() {
        let mut app = App::new(
            config(&[("o/a", &[1]), ("o/b", &[2, 3])]),
            "cfg".into(),
            t(0),
        );
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        let open = vec![(2, Ok(detail(2, vec![]))), (3, Ok(detail(3, vec![])))];
        app.handle(loaded("o/b", open), t(0));
        // Quiet tabs are not polled by themselves...
        assert!(app.tick(t(1000)).is_empty());
        // ...but `r` refreshes every tab.
        app.apply(Action::Refresh);
        assert!(app.refreshing);
        assert_eq!(
            app.tick(t(1001)),
            vec![fetch("o/a", &[1]), fetch_bg("o/b", &[2, 3])]
        );
        assert!(app.refreshing);
        let effects = app.handle(
            loaded(
                "o/b",
                vec![
                    (2, Ok(with_state(2, PrState::Merged, vec![]))),
                    (3, Ok(detail(3, vec![]))),
                ],
            ),
            t(1002),
        );
        assert!(effects.is_empty());
        assert!(app.is_tracked("o/b", 2));
        app.tick(t(1002));
        assert!(app.refreshing, "o/a is still in flight");
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(1002));
        app.tick(t(1003));
        assert!(!app.refreshing);
    }

    #[test]
    fn refresh_retries_a_failed_tab() {
        let mut app = App::new(config(&[("o/a", &[1]), ("o/b", &[2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        let err = ApiError::Network("down".into());
        app.handle(
            Msg::Prs {
                repo: "o/b".into(),
                result: Err(err),
            },
            t(0),
        );
        app.apply(Action::Refresh);
        assert!(app.tick(t(1)).contains(&fetch_bg("o/b", &[2])));
    }

    #[test]
    fn failed_inactive_repo_is_not_polled_in_the_background() {
        let mut app = App::new(config(&[("o/a", &[1]), ("o/b", &[2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        let err = ApiError::NotFound("nope".into());
        app.handle(
            Msg::Prs {
                repo: "o/b".into(),
                result: Err(err),
            },
            t(0),
        );
        assert!(app.tick(t(1000)).is_empty());
    }

    fn notify(title: &str, body: &str) -> Effect {
        Effect::Notify {
            title: title.into(),
            body: body.into(),
        }
    }

    #[test]
    fn finished_checks_notify_once() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        assert!(
            app.handle(loaded("o/a", vec![(1, Ok(running))]), t(0))
                .is_empty()
        );
        let done = detail(1, vec![check("build", CheckState::Pass)]);
        assert_eq!(
            app.handle(loaded("o/a", vec![(1, Ok(done.clone()))]), t(5)),
            vec![notify("Checks passed", "o/a #1 PR 1")]
        );
        assert!(
            app.handle(loaded("o/a", vec![(1, Ok(done))]), t(10))
                .is_empty()
        );
    }

    #[test]
    fn failed_checks_notify_with_the_failure_count() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let running = detail(
            1,
            vec![
                check("build", CheckState::Pending),
                check("lint", CheckState::Fail),
                check("test", CheckState::Pass),
            ],
        );
        app.handle(loaded("o/a", vec![(1, Ok(running))]), t(0));
        let done = detail(
            1,
            vec![
                check("build", CheckState::Fail),
                check("lint", CheckState::Fail),
                check("test", CheckState::Pass),
            ],
        );
        assert_eq!(
            app.handle(loaded("o/a", vec![(1, Ok(done))]), t(5)),
            vec![notify("Checks failed", "o/a #1 PR 1\n2 of 3 checks failed")]
        );
    }

    #[test]
    fn checks_already_complete_or_absent_do_not_notify() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        let done = detail(1, vec![check("build", CheckState::Pass)]);
        let running = detail(2, vec![check("build", CheckState::Pending)]);
        app.handle(loaded("o/a", vec![(1, Ok(done)), (2, Ok(running))]), t(0));
        // PR 2's checks went away (e.g. a push with no workflows).
        let no_checks = detail(2, vec![]);
        let done = detail(1, vec![check("build", CheckState::Pass)]);
        assert!(
            app.handle(loaded("o/a", vec![(1, Ok(done)), (2, Ok(no_checks))]), t(5))
                .is_empty()
        );
    }

    #[test]
    fn rerun_notifies_again() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        let done = detail(1, vec![check("build", CheckState::Pass)]);
        for (i, d) in [&running, &done, &running].into_iter().enumerate() {
            app.handle(loaded("o/a", vec![(1, Ok(d.clone()))]), t(i as i64));
        }
        assert_eq!(
            app.handle(loaded("o/a", vec![(1, Ok(done))]), t(5)),
            vec![notify("Checks passed", "o/a #1 PR 1")]
        );
    }

    #[test]
    fn pr_error_keeps_the_running_state_until_a_later_poll() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        app.handle(loaded("o/a", vec![(1, Ok(running))]), t(0));
        app.handle(loaded("o/a", vec![(1, Err("boom".into()))]), t(5));
        let done = detail(1, vec![check("build", CheckState::Pass)]);
        assert_eq!(
            app.handle(loaded("o/a", vec![(1, Ok(done))]), t(10)).len(),
            1
        );
    }

    #[test]
    fn notification_failure_goes_to_the_status_bar() {
        let mut app = App::new(config(&[]), "cfg".into(), t(0));
        app.handle(Msg::NotifyFailed("cannot notify: no bus".into()), t(0));
        assert_eq!(app.status.as_deref(), Some("cannot notify: no bus"));
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
        // The whole tab is fetched again, whatever was pending.
        assert_eq!(app.tick(t(100)), vec![fetch("o/a", &[1])]);
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

    /// PR 1 with run 7: a failed `lint` job and a running `test` job, both
    /// with steps, plus a passed commit status.
    fn tree_app() -> App {
        let step = |number, state| crate::github::Step {
            number,
            name: format!("step {number}"),
            state,
            started_at: Some(t(0)),
            completed_at: None,
        };
        let mut lint = check("lint", CheckState::Fail);
        lint.run_id = Some(7);
        lint.steps = vec![step(1, CheckState::Fail)];
        let mut test = check("test", CheckState::Pending);
        test.run_id = Some(7);
        test.url = Some("https://github.com/o/r/actions/runs/7/job/2".into());
        test.steps = vec![step(1, CheckState::Pass), step(2, CheckState::Pending)];
        let mut status = check("ext", CheckState::Pass);
        status.kind = CheckKind::Status { description: None };
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        let checks = vec![lint, test, status];
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, checks))), (2, Ok(detail(2, vec![])))],
            ),
            t(0),
        );
        app.focus = Focus::Checks;
        app
    }

    /// Rows of the checks tree, as `name` per row.
    fn outline(app: &App) -> Vec<String> {
        app.repos[0]
            .check_rows()
            .iter()
            .map(|row| match &row.node {
                Node::Run(run) => run.name.to_string(),
                Node::Check(check) => check.name.clone(),
                Node::Step { job, step } => format!("{}/{}", job.name, step.name),
            })
            .collect()
    }

    #[test]
    fn checks_tree_folds_with_arrows_space_and_enter() {
        let mut app = tree_app();
        let all = ["CI", "lint", "test", "test/step 1", "test/step 2", "ext"];
        assert_eq!(outline(&app), all);

        // → expands the failed job; ← on its step goes back to it, then
        // collapses it.
        app.apply(Action::Down);
        app.apply(Action::FocusRight);
        assert_eq!(outline(&app)[2], "lint/step 1");
        app.apply(Action::Down);
        app.apply(Action::FocusLeft);
        assert_eq!(app.repos[0].selected_check, 1);
        app.apply(Action::FocusLeft);
        assert_eq!(outline(&app), all);
        assert_eq!(app.focus, Focus::Checks);

        // Space on a step collapses its job and selects it.
        app.apply(Action::Down);
        app.apply(Action::Down);
        app.apply(Action::Toggle);
        assert_eq!(outline(&app), ["CI", "lint", "test", "ext"]);
        assert_eq!(app.repos[0].selected_check, 2);

        // ← walks up to the run, collapses it, then leaves the pane.
        app.apply(Action::FocusLeft);
        assert_eq!(app.repos[0].selected_check, 0);
        app.apply(Action::FocusLeft);
        assert_eq!(outline(&app), ["CI", "ext"]);
        app.apply(Action::FocusLeft);
        assert_eq!(app.focus, Focus::List);

        // Enter on a run toggles it; overrides survive polls.
        app.apply(Action::FocusRight);
        assert!(app.apply(Action::Enter).is_empty());
        assert_eq!(outline(&app), ["CI", "lint", "test", "ext"]);
        let fresh = tree_app().repos[0].prs[0].detail.clone().unwrap();
        app.handle(loaded("o/a", vec![(1, Ok(fresh))]), t(5));
        assert_eq!(outline(&app), ["CI", "lint", "test", "ext"]);
    }

    #[test]
    fn steps_open_their_job_at_the_step() {
        let mut app = tree_app();
        let step_url = "https://github.com/o/r/actions/runs/7/job/2#step:2:1";
        app.repos[0].selected_check = 4;
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::OpenUrl(step_url.into())]
        );
        assert_eq!(
            app.apply(Action::CopyUrl),
            vec![Effect::CopyToClipboard(
                "https://github.com/o/r/pull/1".into()
            )]
        );
        assert_eq!(
            app.apply(Action::OpenUrl),
            vec![Effect::OpenUrl(step_url.into())]
        );
        // A run opens its run page, cut from its first job's URL.
        app.repos[0].selected_check = 0;
        assert_eq!(
            app.apply(Action::OpenUrl),
            vec![Effect::OpenUrl("https://ci/lint".into())]
        );
        app.repos[0].prs[0].detail.as_mut().unwrap().checks[0].url =
            Some("https://github.com/o/r/actions/runs/7/job/1".into());
        assert_eq!(
            app.apply(Action::OpenUrl),
            vec![Effect::OpenUrl(
                "https://github.com/o/r/actions/runs/7".into()
            )]
        );
        // A completed job's step opens the job's log.
        app.repos[0].selected_check = 1;
        app.apply(Action::FocusRight);
        app.apply(Action::Down);
        assert!(app.apply(Action::Enter).is_empty());
        assert!(matches!(&app.modal, Some(Modal::Log(v)) if v.check.name == "lint"));
    }

    #[test]
    fn selecting_another_pr_refetches_with_its_steps() {
        let mut app = tree_app();
        app.focus = Focus::List;
        assert!(app.tick(t(1)).is_empty());
        app.apply(Action::Down);
        assert_eq!(app.tick(t(1)), vec![fetch_with("o/a", &[1, 2], Some(2))]);
        // Moving within the checks does not refetch.
        app.handle(loaded("o/a", vec![]), t(1));
        app.apply(Action::FocusRight);
        app.apply(Action::Down);
        assert!(app.tick(t(2)).is_empty());
    }

    fn with_state(number: u64, state: PrState, checks: Vec<Check>) -> PrDetail {
        PrDetail {
            state,
            ..detail(number, checks)
        }
    }

    #[test]
    fn closed_and_merged_prs_stay_tracked_and_still_notify() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        let running = detail(1, vec![check("build", CheckState::Pending)]);
        let closed = with_state(2, PrState::Closed, vec![]);
        app.handle(loaded("o/a", vec![(1, Ok(running)), (2, Ok(closed))]), t(0));
        let merged = with_state(1, PrState::Merged, vec![check("build", CheckState::Pass)]);
        assert_eq!(
            app.handle(loaded("o/a", vec![(1, Ok(merged))]), t(30)),
            vec![notify("Checks passed", "o/a #1 PR 1")]
        );
        assert!(app.is_tracked("o/a", 1));
        assert!(app.is_tracked("o/a", 2));
        assert_eq!(app.config, config(&[("o/a", &[1, 2])]));
        assert_eq!(app.status, None);
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
    fn untracking_the_last_pr_empties_the_list_and_then_does_nothing() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        let effects = app.apply(Action::Untrack);
        assert_eq!(effects, vec![Effect::SaveConfig(config(&[("o/a", &[])]))]);
        assert!(app.repos[0].prs.is_empty());
        assert_eq!(app.repos[0].selected_pr, 0);
        assert!(app.apply(Action::Untrack).is_empty());
    }

    #[test]
    fn results_for_an_untracked_pr_are_dropped() {
        let mut app = App::new(config(&[("o/a", &[1, 2])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.apply(Action::Untrack);
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![]))), (2, Ok(detail(2, vec![])))],
            ),
            t(1),
        );
        let numbers: Vec<u64> = app.repos[0].prs.iter().map(|p| p.number).collect();
        assert_eq!(numbers, vec![2]);
        assert!(!app.is_tracked("o/a", 1));
    }

    #[test]
    fn untrack_is_ignored_while_a_modal_is_open() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        open_picker_with(&mut app, &[1, 2]);
        assert!(app.apply(Action::Untrack).is_empty());
        assert!(matches!(app.modal, Some(Modal::Picker(_))));
        app.apply(Action::Close);

        let passed = detail(1, vec![check("build", CheckState::Pass)]);
        app.handle(loaded("o/a", vec![(1, Ok(passed))]), t(1));
        app.apply(Action::FocusRight);
        app.apply(Action::Enter);
        assert!(matches!(app.modal, Some(Modal::Log(_))));
        assert!(app.apply(Action::Untrack).is_empty());
        assert!(matches!(app.modal, Some(Modal::Log(_))));
        assert!(app.is_tracked("o/a", 1));
        assert_eq!(app.repos[0].prs.len(), 1);
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

    #[test]
    fn nothing_is_listed_in_the_background() {
        let mut app = App::new(config(&[("o/a", &[]), ("o/b", &[])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![]), t(0));
        app.handle(loaded("o/b", vec![]), t(0));
        app.apply(Action::Refresh);
        let effects = app.tick(t(1));
        assert_eq!(effects, vec![fetch("o/a", &[]), fetch_bg("o/b", &[])]);
        assert!(app.tick(t(10_000)).is_empty());
    }

    #[test]
    fn a_late_listing_does_not_reset_the_open_picker() {
        let mut app = App::new(config(&[("o/a", &[])]), "cfg".into(), t(0));
        open_picker_with(&mut app, &[1, 2, 3]);
        app.apply(Action::Down);
        app.handle(open_prs(&[4], &[4]), t(2));
        let picker = picker(&app);
        assert_eq!(picker.selected, 1);
        assert_eq!(numbers(picker), vec![1, 2, 3]);
    }

    #[test]
    fn rate_limited_listing_pauses_polling() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.apply(Action::AddPr);
        app.handle(
            Msg::OpenPrs {
                repo: "o/a".into(),
                result: Err(ApiError::RateLimited {
                    reset: Some(t(100)),
                }),
            },
            t(1),
        );
        assert!(app.tick(t(99)).is_empty());
        assert_eq!(app.tick(t(100)).len(), 1);
    }

    fn open_prs(all: &[u64], mine: &[u64]) -> Msg {
        Msg::OpenPrs {
            repo: "o/a".into(),
            result: Ok(OpenPrs {
                all: summaries(all),
                mine: summaries(mine),
            }),
        }
    }

    fn picker(app: &App) -> &Picker {
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("picker closed")
        };
        picker
    }

    fn numbers(picker: &Picker) -> Vec<u64> {
        picker.visible().iter().map(|p| p.number).collect()
    }

    #[test]
    fn picker_opens_on_my_prs_and_m_toggles_all() {
        let mut app = App::new(config(&[("o/a", &[])]), "cfg".into(), t(0));
        app.apply(Action::AddPr);
        app.handle(open_prs(&[5, 4, 3], &[4]), t(1));
        assert!(picker(&app).mine);
        assert_eq!(numbers(picker(&app)), vec![4]);
        app.apply(Action::ToggleMine);
        assert_eq!(numbers(picker(&app)), vec![5, 4, 3]);
        app.apply(Action::Bottom);
        assert_eq!(picker(&app).selected, 2);
        // Toggling back resets the cursor onto the shorter list.
        app.apply(Action::ToggleMine);
        assert_eq!(picker(&app).selected, 0);
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::SaveConfig(config(&[("o/a", &[4])]))]
        );
    }

    #[test]
    fn picker_filters_by_number_prefix() {
        let mut app = App::new(config(&[("o/a", &[])]), "cfg".into(), t(0));
        app.apply(Action::AddPr);
        app.handle(open_prs(&[12, 123, 21, 1], &[]), t(1));
        app.apply(Action::ToggleMine);
        app.apply(Action::Digit(1));
        assert_eq!(numbers(picker(&app)), vec![12, 123, 1]);
        app.apply(Action::Down);
        app.apply(Action::Digit(2));
        assert_eq!(picker(&app).filter, "12");
        assert_eq!(numbers(picker(&app)), vec![12, 123]);
        assert_eq!(picker(&app).selected, 0);
        app.apply(Action::Backspace);
        app.apply(Action::Backspace);
        app.apply(Action::Backspace);
        assert_eq!(picker(&app).filter, "");
        assert_eq!(numbers(picker(&app)).len(), 4);
        // Digits do nothing outside the picker.
        app.apply(Action::Close);
        assert!(app.apply(Action::Digit(1)).is_empty());
        assert!(app.apply(Action::ToggleMine).is_empty());
    }

    #[test]
    fn enter_tracks_the_typed_number_when_nothing_matches() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.apply(Action::AddPr);
        // Works while the listing is still loading...
        app.apply(Action::Digit(4));
        app.apply(Action::Digit(2));
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::SaveConfig(config(&[("o/a", &[1, 42])]))]
        );
        assert_eq!(app.modal, None);
        assert_eq!(app.repos[0].selected().unwrap().number, 42);

        // ...and when the listing has no match.
        app.apply(Action::AddPr);
        app.handle(open_prs(&[5], &[5]), t(1));
        app.apply(Action::Digit(7));
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::SaveConfig(config(&[("o/a", &[1, 42, 7])]))]
        );
        // An already tracked number leaves the picker open.
        app.apply(Action::AddPr);
        app.apply(Action::Digit(1));
        assert!(app.apply(Action::Enter).is_empty());
        assert!(matches!(app.modal, Some(Modal::Picker(_))));
    }

    fn summaries(numbers: &[u64]) -> Vec<PrSummary> {
        numbers
            .iter()
            .map(|n| PrSummary {
                number: *n,
                title: format!("open {n}"),
                author: "x".into(),
                is_draft: false,
            })
            .collect()
    }

    fn open_picker_with(app: &mut App, numbers: &[u64]) {
        assert_eq!(
            app.apply(Action::AddPr),
            vec![Effect::FetchOpenPrs { repo: "o/a".into() }]
        );
        app.handle(open_prs(numbers, numbers), t(1));
    }

    #[test]
    fn picker_adds_the_pr_saves_and_selects_it() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        app.focus = Focus::Checks;
        open_picker_with(&mut app, &[5, 1, 9]);
        app.apply(Action::Down);
        app.apply(Action::Down);
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::SaveConfig(config(&[("o/a", &[1, 9])]))]
        );
        assert_eq!(app.modal, None);
        assert_eq!(app.focus, Focus::List);
        assert_eq!(app.repos[0].selected().unwrap().number, 9);
        assert!(app.is_tracked("o/a", 9));
        // The new PR is fetched right away.
        assert_eq!(app.tick(t(2)), vec![fetch_with("o/a", &[1, 9], Some(9))]);
    }

    #[test]
    fn picking_a_tracked_pr_does_nothing() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        open_picker_with(&mut app, &[1]);
        assert!(app.apply(Action::Enter).is_empty());
        assert!(matches!(app.modal, Some(Modal::Picker(_))));
    }

    #[test]
    fn picker_takes_all_input_and_closes_on_esc_or_q() {
        let mut app = App::new(config(&[("o/a", &[]), ("o/b", &[])]), "cfg".into(), t(0));
        open_picker_with(&mut app, &[1, 2, 3]);
        app.apply(Action::NextRepo);
        assert_eq!(app.active, 0);
        app.apply(Action::Bottom);
        app.apply(Action::Down);
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("picker closed")
        };
        assert_eq!(picker.selected, 2);
        app.apply(Action::Quit);
        assert_eq!(app.modal, None);
        assert!(app.running);

        open_picker_with(&mut app, &[1]);
        app.apply(Action::Close);
        assert_eq!(app.modal, None);
        open_picker_with(&mut app, &[1]);
        app.apply(Action::ForceQuit);
        assert!(!app.running);
    }

    #[test]
    fn picker_shows_listing_errors_and_ignores_stale_results() {
        let mut app = App::new(config(&[("o/a", &[])]), "cfg".into(), t(0));
        app.apply(Action::AddPr);
        app.handle(
            Msg::OpenPrs {
                repo: "o/other".into(),
                result: Ok(OpenPrs::default()),
            },
            t(1),
        );
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("picker closed")
        };
        assert_eq!(picker.items, None);
        app.handle(
            Msg::OpenPrs {
                repo: "o/a".into(),
                result: Err(ApiError::Network("down".into())),
            },
            t(1),
        );
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("picker closed")
        };
        assert!(matches!(&picker.items, Some(Err(e)) if e.contains("down")));
        assert!(app.apply(Action::Enter).is_empty());
    }

    fn api_call(secs: i64, repo: &str) -> Msg {
        let effect = Effect::FetchOpenPrs { repo: repo.into() };
        let request = crate::api_log::Request::of(&effect).unwrap();
        Msg::ApiCall(ApiCall {
            at: t(secs),
            request,
            outcome: Ok(()),
        })
    }

    #[test]
    fn api_calls_are_logged_and_shown_in_a_scrollable_window() {
        let mut app = App::new(config(&[("o/a", &[])]), "cfg".into(), t(0));
        app.log_height = 10;
        for i in 0..30 {
            assert!(app.handle(api_call(i, "o/a"), t(i)).is_empty());
        }
        assert_eq!(app.api_log.len(), 30);

        app.apply(Action::ApiLog);
        let scroll = |app: &App| match &app.modal {
            Some(Modal::ApiLog(scroll)) => scroll.top(app.api_log.len(), 10),
            other => panic!("not the API log: {other:?}"),
        };
        // Opens at the end, following new calls.
        assert_eq!(scroll(&app), 20);
        app.handle(api_call(30, "o/a"), t(30));
        assert_eq!(scroll(&app), 21);
        app.apply(Action::PageUp);
        assert_eq!(scroll(&app), 11);
        app.apply(Action::Top);
        assert_eq!(scroll(&app), 0);
        app.apply(Action::Down);
        assert_eq!(scroll(&app), 1);
        // Other keys are swallowed while it is open.
        assert!(app.apply(Action::AddPr).is_empty());
        assert!(matches!(app.modal, Some(Modal::ApiLog(_))));
        app.apply(Action::Close);
        assert_eq!(app.modal, None);
    }

    #[test]
    fn add_without_repos_explains_what_to_do() {
        let mut app = App::new(Config::default(), "cfg".into(), t(0));
        assert!(app.apply(Action::AddPr).is_empty());
        assert!(app.status.as_deref().unwrap().contains("cfg"));
    }

    fn log_msg(job_id: u64, result: Result<Option<String>, ApiError>) -> Msg {
        Msg::Log {
            repo: "o/a".into(),
            job_id,
            result,
        }
    }

    #[test]
    fn enter_opens_the_pr_and_running_checks_in_the_browser() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        assert!(app.apply(Action::Enter).is_empty());
        let checks = vec![
            check("build", CheckState::Pending),
            check("lint", CheckState::Fail),
        ];
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, checks)))]), t(0));

        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::OpenUrl("https://github.com/o/r/pull/1".into())]
        );
        assert_eq!(app.focus, Focus::List);

        app.apply(Action::FocusRight);
        assert_eq!(
            app.apply(Action::Enter),
            vec![Effect::OpenUrl("https://ci/build".into())]
        );
        assert_eq!(app.modal, None);

        app.apply(Action::Down);
        assert!(app.apply(Action::Enter).is_empty());
        assert!(matches!(&app.modal, Some(Modal::Log(v)) if v.check.name == "lint"));
    }

    #[test]
    fn enter_on_a_pr_without_checks_in_the_checks_pane_does_nothing() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![])))]), t(0));
        app.apply(Action::FocusRight);
        assert!(app.apply(Action::Enter).is_empty());
        assert_eq!(app.modal, None);
    }

    #[test]
    fn enter_opens_the_log_of_a_running_check_without_url_and_polls_it() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.tick(t(0));
        let mut build = check("build", CheckState::Pending);
        build.url = None;
        app.handle(loaded("o/a", vec![(1, Ok(detail(1, vec![build])))]), t(0));

        app.apply(Action::FocusRight);
        assert!(app.apply(Action::Enter).is_empty());
        assert!(matches!(app.modal, Some(Modal::Log(_))));
        let fetch_log = Effect::FetchLog {
            repo: "o/a".into(),
            job_id: job_id("build"),
        };
        assert_eq!(app.tick(t(1)), vec![fetch_log.clone()]);
        app.handle(log_msg(job_id("build"), Ok(None)), t(1));
        let no_log_fetch = |effects: Vec<Effect>| {
            effects
                .iter()
                .all(|e| !matches!(e, Effect::FetchLog { .. }))
        };
        assert!(no_log_fetch(app.tick(t(5))));

        // The PR poll reports the job finished: the log window picks it up
        // and fetches the final log.
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![check("build", CheckState::Pass)])))],
            ),
            t(6),
        );
        assert_eq!(app.tick(t(6)), vec![fetch_log]);
        app.handle(log_msg(job_id("build"), Ok(Some("done\n".into()))), t(6));
        let Some(Modal::Log(view)) = &app.modal else {
            panic!("log closed")
        };
        assert_eq!(view.check.state, CheckState::Pass);
        assert_eq!(view.len(), 1);
        assert!(
            app.tick(t(100))
                .iter()
                .all(|e| !matches!(e, Effect::FetchLog { .. }))
        );
    }

    #[test]
    fn log_window_scrolls_opens_urls_and_closes() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![check("build", CheckState::Pass)])))],
            ),
            t(0),
        );
        app.log_height = 10;
        app.apply(Action::FocusRight);
        app.apply(Action::Enter);
        app.tick(t(0));
        let log: String = (0..50).map(|i| format!("{i}\n")).collect();
        app.handle(log_msg(job_id("build"), Ok(Some(log))), t(0));
        let top = |app: &App| match &app.modal {
            Some(Modal::Log(v)) => v.top(10),
            _ => panic!("log closed"),
        };
        assert_eq!(top(&app), 40);
        app.apply(Action::PageUp);
        assert_eq!(top(&app), 30);
        app.apply(Action::Top);
        assert_eq!(top(&app), 0);
        app.apply(Action::Down);
        assert_eq!(top(&app), 1);
        // Tab keys don't leak through to the main view.
        app.apply(Action::NextRepo);
        assert_eq!(
            app.apply(Action::OpenUrl),
            vec![Effect::OpenUrl("https://ci/build".into())]
        );
        app.apply(Action::Close);
        assert_eq!(app.modal, None);
    }

    #[test]
    fn log_results_for_another_job_are_ignored() {
        let mut app = App::new(config(&[("o/a", &[1])]), "cfg".into(), t(0));
        app.handle(
            loaded(
                "o/a",
                vec![(1, Ok(detail(1, vec![check("build", CheckState::Pass)])))],
            ),
            t(0),
        );
        app.apply(Action::FocusRight);
        app.apply(Action::Enter);
        app.tick(t(0));
        app.handle(log_msg(99, Ok(Some("other".into()))), t(0));
        let Some(Modal::Log(view)) = &app.modal else {
            panic!("log closed")
        };
        assert_eq!(view.len(), 0);
    }
}
