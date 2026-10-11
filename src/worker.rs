//! Runs network effects on the tokio runtime and reports back as [`Msg`]s.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use tokio::runtime::Handle;

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
    /// otherwise hands it back for the caller to execute.
    pub fn spawn(&self, effect: Effect) -> Option<Effect> {
        let github = Arc::clone(&self.github);
        let results = self.results.clone();
        match effect {
            Effect::FetchPrs {
                repo,
                numbers,
                steps,
            } => {
                self.runtime.spawn(async move {
                    let result = github.fetch_prs(&repo, &numbers, steps).await;
                    // The receiver is gone only when the app is quitting.
                    let _ = results.send(Msg::Prs { repo, result });
                });
                None
            }
            Effect::FetchOpenPrs { repo } => {
                self.runtime.spawn(async move {
                    let result = github.open_prs(&repo).await;
                    let _ = results.send(Msg::OpenPrs { repo, result });
                });
                None
            }
            Effect::FetchViewer => {
                self.runtime.spawn(async move {
                    let result = github.viewer_login().await;
                    let _ = results.send(Msg::Viewer(result));
                });
                None
            }
            Effect::FetchLog { repo, job_id } => {
                self.runtime.spawn(async move {
                    let result = github.job_log(&repo, job_id).await;
                    let _ = results.send(Msg::Log {
                        repo,
                        job_id,
                        result,
                    });
                });
                None
            }
            other => Some(other),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::github::{ApiError, PrResult, PrSummary};

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

        async fn open_prs(&self, _repo: &str) -> Result<Vec<PrSummary>, ApiError> {
            Ok(vec![])
        }

        async fn viewer_login(&self) -> Result<String, ApiError> {
            Ok("me".into())
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
    fn viewer_login_comes_back_as_a_message() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(FakeGitHub, runtime.handle().clone(), tx);
        assert_eq!(worker.spawn(Effect::FetchViewer), None);
        let msg = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(msg, Msg::Viewer(Ok("me".into())));
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
