//! [`GitHub`] implemented with octocrab.

use http::StatusCode;
use octocrab::Octocrab;
use serde_json::{Value, json};

use super::{ApiError, GitHub, PrResult, PrSummary, parse, query};

pub struct OctoGitHub {
    crab: Octocrab,
}

impl OctoGitHub {
    /// Must be called inside a tokio runtime.
    pub fn new(token: String) -> Result<Self, ApiError> {
        let crab = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(map_err)?;
        Ok(Self { crab })
    }

    async fn graphql(&self, query: &str, repo: &str) -> Result<Value, ApiError> {
        let (owner, name) = split_repo(repo)?;
        let body = json!({ "query": query, "variables": { "owner": owner, "name": name } });
        self.crab
            .post("/graphql", Some(&body))
            .await
            .map_err(map_err)
    }
}

impl GitHub for OctoGitHub {
    async fn fetch_prs(&self, repo: &str, numbers: &[u64]) -> Result<Vec<PrResult>, ApiError> {
        let response = self.graphql(&query::prs(numbers), repo).await?;
        parse::prs(&response, numbers)
    }

    async fn open_prs(&self, repo: &str) -> Result<Vec<PrSummary>, ApiError> {
        let response = self.graphql(query::OPEN_PRS, repo).await?;
        parse::open_prs(&response)
    }

    async fn viewer_login(&self) -> Result<String, ApiError> {
        let user = self.crab.current().user().await.map_err(map_err)?;
        Ok(user.login)
    }

    async fn job_log(&self, repo: &str, job_id: u64) -> Result<Option<String>, ApiError> {
        split_repo(repo)?;
        let route = format!("/repos/{repo}/actions/jobs/{job_id}/logs");
        let response = self.crab._get(route.as_str()).await.map_err(map_err)?;
        // The endpoint redirects to a short-lived download URL.
        let response = self
            .crab
            .follow_location_to_data(response)
            .await
            .map_err(map_err)?;
        match octocrab::map_github_error(response).await {
            Ok(response) => self
                .crab
                .body_to_string(response)
                .await
                .map(Some)
                .map_err(map_err),
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code == StatusCode::NOT_FOUND =>
            {
                Ok(None)
            }
            Err(e) => Err(map_err(e)),
        }
    }
}

fn split_repo(repo: &str) -> Result<(&str, &str), ApiError> {
    repo.split_once('/')
        .filter(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('/'))
        .ok_or_else(|| ApiError::Other(format!("invalid repo name {repo:?}, expected owner/name")))
}

fn map_err(error: octocrab::Error) -> ApiError {
    match error {
        octocrab::Error::GitHub { source, .. } => {
            let rate_limited = source.status_code == StatusCode::TOO_MANY_REQUESTS
                || (source.status_code == StatusCode::FORBIDDEN
                    && source.message.to_lowercase().contains("rate limit"));
            if rate_limited {
                ApiError::RateLimited {
                    reset: source.rate_limit_reset_at(),
                }
            } else {
                match source.status_code {
                    StatusCode::NOT_FOUND => ApiError::NotFound(source.message),
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                        ApiError::Forbidden(source.message)
                    }
                    code => ApiError::Other(format!("{code}: {}", source.message)),
                }
            }
        }
        e @ (octocrab::Error::Hyper { .. }
        | octocrab::Error::Service { .. }
        | octocrab::Error::Http { .. }) => ApiError::Network(e.to_string()),
        e => ApiError::Other(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_names_must_be_owner_slash_name() {
        assert_eq!(split_repo("a/b").unwrap(), ("a", "b"));
        assert!(split_repo("a").is_err());
        assert!(split_repo("/b").is_err());
        assert!(split_repo("a/b/c").is_err());
    }

    /// Hits the real API: `cargo test -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_smoke_test() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let token = crate::auth::token().expect("token");
            let gh = OctoGitHub::new(token).unwrap();
            let prs = gh.fetch_prs("fpacifici/assistant", &[69]).await.unwrap();
            let pr = prs[0].1.as_ref().unwrap();
            let job_id = pr
                .checks
                .iter()
                .find_map(|c| match c.kind {
                    super::super::CheckKind::ActionsJob { job_id } => Some(job_id),
                    _ => None,
                })
                .unwrap();
            let log = gh.job_log("fpacifici/assistant", job_id).await.unwrap();
            assert!(log.unwrap().contains("Current runner version"));
            let missing = gh.fetch_prs("fpacifici/does-not-exist-xyz", &[]).await;
            assert!(matches!(missing, Err(ApiError::NotFound(_))), "{missing:?}");
            gh.open_prs("ratatui/ratatui").await.unwrap();
            assert!(!gh.viewer_login().await.unwrap().is_empty());
        });
    }
}
