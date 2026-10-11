//! Runs network effects on the tokio runtime and reports back as [`Msg`]s.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use chrono::Utc;
use tokio::runtime::Handle;

use crate::api_log::{ApiCall, Request};
use crate::app::{Effect, Msg};
use crate::github::GitHub;

pub struct Worker<G> {
    github: Arc<G>,
    runtime: Handle,
    results: Sender<Msg>,
}

impl<G: GitHub> Worker<G> {
    pub fn new(github: G, runtime: Handle, results: Sender<Msg>) -> Self {
        Self {
            github: Arc::new(github),
            runtime,
            results,
        }
    }

    /// Spawns `effect` if it is a network effect and returns `None`;
    /// otherwise hands it back for the caller to execute. Each request is
    /// reported as a [`Msg::ApiCall`] before its result.
    pub fn spawn(&self, effect: Effect) -> Option<Effect> {
        let Some(request) = Request::of(&effect) else {
            return Some(effect);
        };
        let github = Arc::clone(&self.github);
        let results = self.results.clone();
        let at = Utc::now();
        self.runtime.spawn(async move {
            let (call, msg) = match effect {
                Effect::FetchPrs {
                    repo,
                    numbers,
                    steps,
                } => {
                    let result = github.fetch_prs(&repo, &numbers, steps).await;
                    let call = ApiCall::new(at, request, &result);
                    (call, Msg::Prs { repo, result })
                }
                Effect::FetchOpenPrs { repo } => {
                    let result = github.open_prs(&repo).await;
                    let call = ApiCall::new(at, request, &result);
                    (call, Msg::OpenPrs { repo, result })
                }
                Effect::FetchLog { repo, job_id } => {
                    let result = github.job_log(&repo, job_id).await;
                    let call = ApiCall::new(at, request, &result);
                    let msg = Msg::Log {
                        repo,
                        job_id,
                        result,
                    };
                    (call, msg)
                }
                Effect::SaveConfig(_)
                | Effect::CopyToClipboard(_)
                | Effect::OpenUrl(_)
                | Effect::Notify { .. } => unreachable!("local effects have no request"),
            };
            // The receiver is gone only when the app is quitting.
            let _ = results.send(Msg::ApiCall(call));
            let _ = results.send(msg);
        });
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::github::{ApiError, OpenPrs, PrResult};

    /// Answers every request from canned data.
    pub struct FakeGitHub;

    impl GitHub for FakeGitHub {
        async fn fetch_prs(
            &self,
            repo: &str,
            numbers: &[u64],
            _steps: Option<u64>,
        ) -> Result<Vec<PrResult>, ApiError> {
            if repo == "o/missing" {
                return Err(ApiError::NotFound(repo.into()));
            }
            Ok(numbers
                .iter()
                .map(|n| (*n, Err(format!("fake {n}"))))
                .collect())
        }

        async fn open_prs(&self, _repo: &str) -> Result<OpenPrs, ApiError> {
            Ok(OpenPrs::default())
        }

        async fn job_log(&self, _repo: &str, job_id: u64) -> Result<Option<String>, ApiError> {
            Ok(Some(format!("log {job_id}")))
        }
    }

    #[test]
    fn network_effects_come_back_as_messages() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(FakeGitHub, runtime.handle().clone(), tx);

        let left = worker.spawn(Effect::FetchPrs {
            repo: "o/a".into(),
            numbers: vec![1],
            steps: Some(1),
        });
        assert_eq!(left, None);
        let Msg::ApiCall(call) = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("the call is reported first")
        };
        assert_eq!(call.request.what, "PRs o/a #1 (steps #1)");
        assert_eq!(call.outcome, Ok(()));
        let msg = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            msg,
            Msg::Prs {
                repo: "o/a".into(),
                result: Ok(vec![(1, Err("fake 1".into()))]),
            }
        );
    }

    #[test]
    fn open_prs_come_back_as_a_message() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(FakeGitHub, runtime.handle().clone(), tx);
        let effect = Effect::FetchOpenPrs { repo: "o/a".into() };
        assert_eq!(worker.spawn(effect), None);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Msg::ApiCall(_)
        ));
        let msg = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            msg,
            Msg::OpenPrs {
                repo: "o/a".into(),
                result: Ok(OpenPrs::default()),
            }
        );
    }

    #[test]
    fn failed_calls_are_logged_with_their_code() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(FakeGitHub, runtime.handle().clone(), tx);
        worker.spawn(Effect::FetchPrs {
            repo: "o/missing".into(),
            numbers: vec![],
            steps: None,
        });
        let Msg::ApiCall(call) = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("the call is reported first")
        };
        assert_eq!(call.outcome, Err("not found".into()));
    }

    #[test]
    fn local_effects_are_handed_back() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, _rx) = mpsc::channel();
        let worker = Worker::new(FakeGitHub, runtime.handle().clone(), tx);
        let effect = Effect::OpenUrl("u".into());
        assert_eq!(worker.spawn(effect.clone()), Some(effect));
    }
}
