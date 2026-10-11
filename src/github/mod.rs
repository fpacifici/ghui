//! GitHub access: domain types, the [`GitHub`] trait and its octocrab
//! implementation. Parsing is kept pure in [`parse`] so it can be tested
//! against recorded fixtures.

use std::future::Future;

use chrono::{DateTime, Utc};

pub mod octo;
pub mod parse;
pub mod query;

/// The subset of the GitHub API ghui needs.
pub trait GitHub: Send + Sync + 'static {
    /// Fetches the given PRs of `repo` (`owner/name`) in one request, with
    /// the job steps of PR `steps` only, since steps are expensive. The
    /// outer error is repo-wide (not found, no access, network); the inner
    /// one is per PR.
    fn fetch_prs(
        &self,
        repo: &str,
        numbers: &[u64],
        steps: Option<u64>,
    ) -> impl Future<Output = Result<Vec<PrResult>, ApiError>> + Send;

    /// The 50 most recently updated open PRs of `repo`, and the user's.
    fn open_prs(&self, repo: &str) -> impl Future<Output = Result<OpenPrs, ApiError>> + Send;

    /// The plain-text log of an Actions job. `Ok(None)` means the log is
    /// not available (yet).
    fn job_log(
        &self,
        repo: &str,
        job_id: u64,
    ) -> impl Future<Output = Result<Option<String>, ApiError>> + Send;
}

/// One requested PR: its number and either its data or why it failed.
pub type PrResult = (u64, Result<PrDetail, String>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    NotFound(String),
    Forbidden(String),
    RateLimited { reset: Option<DateTime<Utc>> },
    Network(String),
    Other(String),
}

impl ApiError {
    /// A short error code for the API log, without the message.
    pub fn code(&self) -> String {
        match self {
            ApiError::NotFound(_) => "not found".into(),
            ApiError::Forbidden(_) => "forbidden".into(),
            ApiError::RateLimited { .. } => "rate limited".into(),
            ApiError::Network(_) => "network error".into(),
            // HTTP failures read `502 Bad Gateway: …`; GraphQL ones carry
            // only a message.
            ApiError::Other(m) => match m.get(..3) {
                Some(code) if code.bytes().all(|b| b.is_ascii_digit()) => code.into(),
                _ => "error".into(),
            },
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound(m) => write!(f, "not found or no access: {m}"),
            ApiError::Forbidden(m) => write!(f, "forbidden: {m}"),
            ApiError::RateLimited { reset: Some(at) } => {
                write!(f, "rate limited until {}", at.format("%H:%M:%S UTC"))
            }
            ApiError::RateLimited { reset: None } => write!(f, "rate limited"),
            ApiError::Network(m) => write!(f, "network error: {m}"),
            ApiError::Other(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

/// Status of a single check, as `gh pr checks` buckets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckState {
    // Declaration order is the display order.
    Fail,
    Pending,
    Pass,
    Skipped,
}

impl CheckState {
    pub fn glyph(self) -> &'static str {
        match self {
            CheckState::Pass => "✓",
            CheckState::Fail => "X",
            CheckState::Pending => "*",
            CheckState::Skipped => "-",
        }
    }
}

/// Aggregate state of all checks on a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rollup {
    Success,
    Failure,
    Pending,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckKind {
    /// A GitHub Actions job; `job_id` addresses its log.
    ActionsJob { job_id: u64 },
    /// A check run from another app.
    CheckRun {
        title: Option<String>,
        summary: Option<String>,
    },
    /// A legacy commit status.
    Status { description: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    /// Workflow name for Actions jobs; empty for others.
    pub workflow: Option<String>,
    pub state: CheckState,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub url: Option<String>,
    pub kind: CheckKind,
    /// The Actions workflow run of a job; jobs are grouped by it.
    pub run_id: Option<u64>,
    /// An Actions job's steps in execution order; empty for other checks
    /// and when steps were not requested.
    pub steps: Vec<Step>,
}

impl Check {
    pub fn is_running(&self) -> bool {
        self.state == CheckState::Pending
    }

    pub fn job_id(&self) -> Option<u64> {
        match self.kind {
            CheckKind::ActionsJob { job_id } => Some(job_id),
            _ => None,
        }
    }

    /// The job's page scrolled to step `number`, for Actions jobs.
    pub fn step_url(&self, number: u64) -> Option<String> {
        let url = self.url.as_ref()?;
        Some(match self.kind {
            CheckKind::ActionsJob { .. } => format!("{url}#step:{number}:1"),
            _ => url.clone(),
        })
    }
}

/// A step of an Actions job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub number: u64,
    pub name: String,
    pub state: CheckState,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub state: PrState,
    pub author: String,
    pub head: String,
    pub base: String,
    /// Sorted failing → pending → passing → skipped, then by name.
    pub checks: Vec<Check>,
}

impl PrDetail {
    pub fn rollup(&self) -> Rollup {
        if self.checks.is_empty() {
            Rollup::None
        } else if self.checks.iter().any(|c| c.state == CheckState::Fail) {
            Rollup::Failure
        } else if self.checks.iter().any(|c| c.state == CheckState::Pending) {
            Rollup::Pending
        } else {
            Rollup::Success
        }
    }

    pub fn checks_running(&self) -> bool {
        self.checks.iter().any(Check::is_running)
    }
}

/// The add-PR picker's listings of a repo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenPrs {
    /// The 50 most recently updated open PRs.
    pub all: Vec<PrSummary>,
    /// Open PRs authored by the user, most recently updated first.
    pub mine: Vec<PrSummary>,
}

/// A row of the add-PR picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub is_draft: bool,
}
