//! The session's log of GitHub API calls; see spec 0009. Pure, like
//! [`crate::app`]: the worker stamps and reports calls, the app keeps them.

use std::collections::VecDeque;

use chrono::{DateTime, Utc};

use crate::app::Effect;
use crate::github::ApiError;

/// Calls kept; older ones are dropped.
pub const CAPACITY: usize = 1000;

/// What was asked of the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub path: String,
    /// What the request fetches, e.g. `PRs o/a #1 #2 (steps #1)`.
    pub what: String,
}

impl Request {
    /// The request a network effect makes; `None` for local effects.
    pub fn of(effect: &Effect) -> Option<Self> {
        let graphql = |what: String| Request {
            method: "POST",
            path: "/graphql".into(),
            what,
        };
        match effect {
            Effect::FetchPrs { repo, numbers, .. } if numbers.is_empty() => {
                Some(graphql(format!("repo {repo}")))
            }
            Effect::FetchPrs {
                repo,
                numbers,
                steps,
            } => {
                let mut what = format!("PRs {repo}");
                for n in numbers {
                    what.push_str(&format!(" #{n}"));
                }
                if let Some(n) = steps {
                    what.push_str(&format!(" (steps #{n})"));
                }
                Some(graphql(what))
            }
            Effect::FetchOpenPrs { repo } => Some(graphql(format!("open PRs {repo}"))),
            Effect::FetchLog { repo, job_id } => Some(Request {
                method: "GET",
                path: format!("/repos/{repo}/actions/jobs/{job_id}/logs"),
                what: "job log".into(),
            }),
            Effect::SaveConfig(_)
            | Effect::CopyToClipboard(_)
            | Effect::OpenUrl(_)
            | Effect::Notify { .. } => None,
        }
    }
}

/// A completed API call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCall {
    /// When it was sent.
    pub at: DateTime<Utc>,
    pub request: Request,
    /// `Err` holds the error code ([`ApiError::code`]).
    pub outcome: Result<(), String>,
}

impl ApiCall {
    pub fn new<T>(at: DateTime<Utc>, request: Request, result: &Result<T, ApiError>) -> Self {
        Self {
            at,
            request,
            outcome: result.as_ref().map(|_| ()).map_err(ApiError::code),
        }
    }
}

/// The last [`CAPACITY`] calls, oldest first.
#[derive(Debug, Default)]
pub struct ApiLog {
    calls: VecDeque<ApiCall>,
}

impl ApiLog {
    pub fn push(&mut self, call: ApiCall) {
        if self.calls.len() == CAPACITY {
            self.calls.pop_front();
        }
        self.calls.push_back(call);
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ApiCall> {
        self.calls.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn describes_network_effects() {
        let prs = Effect::FetchPrs {
            repo: "o/a".into(),
            numbers: vec![1, 2],
            steps: Some(1),
        };
        let request = Request::of(&prs).unwrap();
        assert_eq!(
            (request.method, request.path.as_str()),
            ("POST", "/graphql")
        );
        assert_eq!(request.what, "PRs o/a #1 #2 (steps #1)");

        let empty = Effect::FetchPrs {
            repo: "o/a".into(),
            numbers: vec![],
            steps: None,
        };
        assert_eq!(Request::of(&empty).unwrap().what, "repo o/a");

        let open = Effect::FetchOpenPrs { repo: "o/a".into() };
        assert_eq!(Request::of(&open).unwrap().what, "open PRs o/a");

        let log = Request::of(&Effect::FetchLog {
            repo: "o/a".into(),
            job_id: 7,
        })
        .unwrap();
        assert_eq!(log.method, "GET");
        assert_eq!(log.path, "/repos/o/a/actions/jobs/7/logs");

        assert_eq!(Request::of(&Effect::OpenUrl("u".into())), None);
    }

    #[test]
    fn outcome_keeps_only_the_error_code() {
        let request = Request::of(&Effect::FetchOpenPrs { repo: "o/a".into() }).unwrap();
        let ok: Result<u8, ApiError> = Ok(1);
        assert_eq!(ApiCall::new(at(0), request.clone(), &ok).outcome, Ok(()));
        let limited: Result<u8, ApiError> = Err(ApiError::RateLimited { reset: None });
        assert_eq!(
            ApiCall::new(at(0), request, &limited).outcome,
            Err("rate limited".into())
        );
    }

    #[test]
    fn error_codes_drop_the_message() {
        let code = |e: ApiError| e.code();
        assert_eq!(code(ApiError::NotFound("x".into())), "not found");
        assert_eq!(code(ApiError::Forbidden("x".into())), "forbidden");
        assert_eq!(code(ApiError::Network("x".into())), "network error");
        assert_eq!(code(ApiError::Other("502 Bad Gateway: x".into())), "502");
        assert_eq!(code(ApiError::Other("Something broke".into())), "error");
        assert_eq!(code(ApiError::Other("".into())), "error");
    }

    #[test]
    fn keeps_the_last_calls() {
        let request = Request::of(&Effect::FetchOpenPrs { repo: "o/a".into() }).unwrap();
        let mut log = ApiLog::default();
        for i in 0..=CAPACITY as i64 {
            log.push(ApiCall {
                at: at(i),
                request: request.clone(),
                outcome: Ok(()),
            });
        }
        assert_eq!(log.len(), CAPACITY);
        assert_eq!(log.iter().next().unwrap().at, at(1));
    }
}
