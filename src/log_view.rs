//! State of the log window opened on a check: content, scroll position and
//! the polling bookkeeping for running jobs. Pure, like [`crate::app`].

use chrono::{DateTime, TimeDelta, Utc};

use crate::github::{Check, CheckKind, CheckState};
use crate::logs;

/// Re-fetch interval for the log of a running job.
pub const LOG_POLL: TimeDelta = TimeDelta::seconds(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogContent {
    Loading,
    /// The job is still running and GitHub has no log for it yet.
    NotYet,
    /// The job finished but GitHub returned no log (e.g. expired).
    Unavailable,
    Lines(Vec<String>),
    Error(String),
    /// Not an Actions job: show the check's own summary instead.
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogView {
    pub repo: String,
    pub pr: u64,
    /// Kept up to date from the PR polls while the window is open.
    pub check: Check,
    pub content: LogContent,
    /// Index of the top visible line when not following.
    scroll: usize,
    /// Stick to the end of the log (`tail -f`).
    follow: bool,
    in_flight: bool,
    force: bool,
    last_fetch: Option<DateTime<Utc>>,
    /// Whether the in-flight request was made after the job completed, so
    /// its answer is final.
    requested_final: bool,
    complete: bool,
}

impl LogView {
    pub fn new(repo: String, pr: u64, check: Check) -> Self {
        let is_job = matches!(check.kind, CheckKind::ActionsJob { .. });
        Self {
            repo,
            pr,
            check,
            content: if is_job {
                LogContent::Loading
            } else {
                LogContent::Summary
            },
            scroll: 0,
            follow: true,
            in_flight: false,
            force: false,
            last_fetch: None,
            requested_final: false,
            complete: !is_job,
        }
    }

    pub fn job_id(&self) -> Option<u64> {
        match self.check.kind {
            CheckKind::ActionsJob { job_id } => Some(job_id),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        match &self.content {
            LogContent::Lines(lines) => lines.len(),
            _ => 0,
        }
    }

    /// The top visible line for a viewport of `height` lines.
    pub fn top(&self, height: usize) -> usize {
        let max = self.len().saturating_sub(height);
        if self.follow {
            max
        } else {
            self.scroll.min(max)
        }
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Scrolls by `delta` lines; reaching the end turns following back on.
    pub fn scroll_by(&mut self, delta: isize, height: usize) {
        let max = self.len().saturating_sub(height);
        let top = self.top(height).saturating_add_signed(delta).min(max);
        self.scroll = top;
        self.follow = top == max;
    }

    /// Whether a log request is due at `now`.
    pub fn poll_due(&self, now: DateTime<Utc>) -> bool {
        self.job_id().is_some()
            && !self.complete
            && !self.in_flight
            && (self.force || self.last_fetch.is_none_or(|last| now >= last + LOG_POLL))
    }

    /// Records that a request was sent.
    pub fn start_fetch(&mut self) {
        self.in_flight = true;
        self.force = false;
        self.requested_final = !self.check.is_running();
    }

    /// `r`: fetch again now, even if the log looked final.
    pub fn refresh(&mut self) {
        self.force = true;
        self.complete = !matches!(self.check.kind, CheckKind::ActionsJob { .. });
    }

    /// Applies a fetch result. `Err` carries a display message.
    pub fn finish_fetch(&mut self, result: Result<Option<String>, String>, now: DateTime<Utc>) {
        self.in_flight = false;
        self.last_fetch = Some(now);
        let final_answer = self.requested_final;
        match result {
            Ok(Some(raw)) => {
                let lines = logs::clean(&raw);
                if !matches!(self.content, LogContent::Lines(_)) {
                    self.initial_position(&lines);
                }
                self.content = LogContent::Lines(lines);
                self.complete = final_answer;
            }
            Ok(None) => {
                self.content = if final_answer {
                    LogContent::Unavailable
                } else {
                    LogContent::NotYet
                };
                self.complete = final_answer;
            }
            // Keep showing what we have; the next poll retries.
            Err(_) if matches!(self.content, LogContent::Lines(_)) => {}
            Err(message) => self.content = LogContent::Error(message),
        }
    }

    /// Failed jobs open at the first error, others at the end.
    fn initial_position(&mut self, lines: &[String]) {
        match logs::first_error(lines) {
            Some(index) if self.check.state == CheckState::Fail => {
                // A little context above the error.
                self.scroll = index.saturating_sub(3);
                self.follow = false;
            }
            _ => self.follow = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{check, t};

    fn view(state: CheckState) -> LogView {
        LogView::new("o/a".into(), 1, check("build", state))
    }

    fn log(lines: usize, error_at: Option<usize>) -> String {
        (0..lines)
            .map(|i| {
                if Some(i) == error_at {
                    "##[error]boom\n".to_string()
                } else {
                    format!("line {i}\n")
                }
            })
            .collect()
    }

    #[test]
    fn running_job_is_polled_until_a_fetch_after_completion() {
        let mut v = view(CheckState::Pending);
        assert!(v.poll_due(t(0)));
        v.start_fetch();
        assert!(!v.poll_due(t(0)));
        v.finish_fetch(Ok(None), t(0));
        assert_eq!(v.content, LogContent::NotYet);
        assert!(!v.poll_due(t(4)));
        assert!(v.poll_due(t(5)));

        v.start_fetch();
        v.finish_fetch(Ok(Some(log(3, None))), t(5));
        assert!(v.poll_due(t(10)));

        // The job completes; one more fetch, then polling stops.
        v.check.state = CheckState::Pass;
        v.start_fetch();
        v.finish_fetch(Ok(Some(log(4, None))), t(10));
        assert!(!v.poll_due(t(100)));
        v.refresh();
        assert!(v.poll_due(t(100)));
    }

    #[test]
    fn finished_job_without_log_is_unavailable() {
        let mut v = view(CheckState::Pass);
        v.start_fetch();
        v.finish_fetch(Ok(None), t(0));
        assert_eq!(v.content, LogContent::Unavailable);
        assert!(!v.poll_due(t(100)));
    }

    #[test]
    fn non_actions_checks_are_never_fetched() {
        let mut c = check("ext", CheckState::Pending);
        c.kind = CheckKind::Status { description: None };
        let v = LogView::new("o/a".into(), 1, c);
        assert_eq!(v.content, LogContent::Summary);
        assert!(!v.poll_due(t(0)));
    }

    #[test]
    fn follows_the_end_and_stops_when_scrolled_up() {
        let mut v = view(CheckState::Pending);
        v.start_fetch();
        v.finish_fetch(Ok(Some(log(100, None))), t(0));
        assert!(v.is_following());
        assert_eq!(v.top(10), 90);

        v.scroll_by(-5, 10);
        assert!(!v.is_following());
        assert_eq!(v.top(10), 85);
        // New output does not move a scrolled view.
        v.start_fetch();
        v.finish_fetch(Ok(Some(log(150, None))), t(5));
        assert_eq!(v.top(10), 85);

        v.scroll_by(isize::MAX, 10);
        assert!(v.is_following());
        assert_eq!(v.top(10), 140);
        v.scroll_by(isize::MIN, 10);
        assert_eq!(v.top(10), 0);
    }

    #[test]
    fn failed_job_opens_at_the_first_error() {
        let mut v = view(CheckState::Fail);
        v.start_fetch();
        v.finish_fetch(Ok(Some(log(100, Some(40)))), t(0));
        assert!(!v.is_following());
        assert_eq!(v.top(10), 37);
    }

    #[test]
    fn errors_keep_existing_lines() {
        let mut v = view(CheckState::Pending);
        v.start_fetch();
        v.finish_fetch(Err("down".into()), t(0));
        assert_eq!(v.content, LogContent::Error("down".into()));
        v.start_fetch();
        v.finish_fetch(Ok(Some(log(2, None))), t(5));
        v.start_fetch();
        v.finish_fetch(Err("down".into()), t(10));
        assert_eq!(v.len(), 2);
    }
}
