//! Pure conversion of GraphQL responses into domain types.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::query::pr_alias;
use super::{ApiError, Check, CheckKind, CheckState, PrDetail, PrResult, PrState, PrSummary};

/// Parses the response to [`super::query::prs`].
pub fn prs(response: &Value, numbers: &[u64]) -> Result<Vec<PrResult>, ApiError> {
    let errors = graphql_errors(response);
    let repo = repository(response, &errors)?;
    Ok(numbers
        .iter()
        .map(|&n| {
            let alias = pr_alias(n);
            let result = match repo.get(&alias) {
                Some(v) if !v.is_null() => pr_detail(v),
                _ => Err(errors
                    .iter()
                    .find(|e| e.path.get(1).and_then(Value::as_str) == Some(alias.as_str()))
                    // The full message is too long for the narrow PR list.
                    .map(|e| match e.kind.as_deref() {
                        Some("NOT_FOUND") => "not found".to_string(),
                        _ => e.message.clone(),
                    })
                    .unwrap_or_else(|| format!("PR #{n} not returned"))),
            };
            (n, result)
        })
        .collect())
}

/// Parses the response to [`super::query::OPEN_PRS`].
pub fn open_prs(response: &Value) -> Result<Vec<PrSummary>, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        number: u64,
        title: String,
        #[serde(default)]
        is_draft: bool,
        author: Option<Login>,
    }

    let errors = graphql_errors(response);
    let repo = repository(response, &errors)?;
    let nodes = repo
        .pointer("/pullRequests/nodes")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let raw: Vec<Raw> =
        serde_json::from_value(nodes).map_err(|e| ApiError::Other(e.to_string()))?;
    Ok(raw
        .into_iter()
        .map(|r| PrSummary {
            number: r.number,
            title: r.title,
            author: login(r.author),
            is_draft: r.is_draft,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    path: Vec<Value>,
    message: String,
}

fn graphql_errors(response: &Value) -> Vec<GraphqlError> {
    response
        .get("errors")
        .and_then(|e| serde_json::from_value(e.clone()).ok())
        .unwrap_or_default()
}

/// Returns `data.repository`, or the repo-wide error explaining its absence.
fn repository<'a>(response: &'a Value, errors: &[GraphqlError]) -> Result<&'a Value, ApiError> {
    if errors
        .iter()
        .any(|e| e.kind.as_deref() == Some("RATE_LIMITED"))
    {
        return Err(ApiError::RateLimited { reset: None });
    }
    match response.pointer("/data/repository") {
        Some(repo) if !repo.is_null() => Ok(repo),
        _ => {
            let err = errors
                .iter()
                .find(|e| e.path.first().and_then(Value::as_str) == Some("repository"))
                .or(errors.first());
            Err(match err {
                Some(e) => match e.kind.as_deref() {
                    Some("NOT_FOUND") => ApiError::NotFound(e.message.clone()),
                    Some("FORBIDDEN") => ApiError::Forbidden(e.message.clone()),
                    _ => ApiError::Other(e.message.clone()),
                },
                None => ApiError::Other("response has no repository".into()),
            })
        }
    }
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

fn login(author: Option<Login>) -> String {
    // Deleted accounts come back as null.
    author.map_or_else(|| "ghost".into(), |a| a.login)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPr {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    url: String,
    state: String,
    author: Option<Login>,
    head_ref_name: String,
    base_ref_name: String,
    commits: RawCommits,
}

#[derive(Deserialize)]
struct RawCommits {
    nodes: Vec<RawCommitNode>,
}

#[derive(Deserialize)]
struct RawCommitNode {
    commit: RawCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCommit {
    status_check_rollup: Option<RawRollup>,
}

#[derive(Deserialize)]
struct RawRollup {
    contexts: RawContexts,
}

#[derive(Deserialize)]
struct RawContexts {
    nodes: Vec<RawContext>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum RawContext {
    #[serde(rename_all = "camelCase")]
    CheckRun {
        database_id: Option<u64>,
        name: String,
        status: String,
        conclusion: Option<String>,
        started_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        details_url: Option<String>,
        title: Option<String>,
        summary: Option<String>,
        check_suite: Option<RawCheckSuite>,
    },
    #[serde(rename_all = "camelCase")]
    StatusContext {
        context: String,
        state: String,
        description: Option<String>,
        target_url: Option<String>,
        created_at: Option<DateTime<Utc>>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCheckSuite {
    app: Option<RawApp>,
    workflow_run: Option<RawWorkflowRun>,
}

#[derive(Deserialize)]
struct RawApp {
    slug: String,
}

#[derive(Deserialize)]
struct RawWorkflowRun {
    workflow: RawWorkflow,
}

#[derive(Deserialize)]
struct RawWorkflow {
    name: String,
}

fn pr_detail(value: &Value) -> Result<PrDetail, String> {
    let raw: RawPr = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let mut checks: Vec<Check> = raw
        .commits
        .nodes
        .into_iter()
        .filter_map(|n| n.commit.status_check_rollup)
        .flat_map(|r| r.contexts.nodes)
        .map(check)
        .collect();
    checks.sort_by(|a, b| a.state.cmp(&b.state).then_with(|| a.name.cmp(&b.name)));
    Ok(PrDetail {
        number: raw.number,
        title: raw.title,
        body: raw.body,
        url: raw.url,
        state: match raw.state.as_str() {
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ => PrState::Open,
        },
        author: login(raw.author),
        head: raw.head_ref_name,
        base: raw.base_ref_name,
        checks,
    })
}

fn check(raw: RawContext) -> Check {
    match raw {
        RawContext::CheckRun {
            database_id,
            name,
            status,
            conclusion,
            started_at,
            completed_at,
            details_url,
            title,
            summary,
            check_suite,
        } => {
            let (slug, workflow) = check_suite
                .map(|s| {
                    (
                        s.app.map(|a| a.slug),
                        s.workflow_run.map(|w| w.workflow.name),
                    )
                })
                .unwrap_or_default();
            let kind = match (slug.as_deref(), database_id) {
                (Some("github-actions"), Some(job_id)) => CheckKind::ActionsJob { job_id },
                _ => CheckKind::CheckRun { title, summary },
            };
            Check {
                name,
                workflow,
                state: check_run_state(&status, conclusion.as_deref()),
                started_at,
                completed_at,
                url: details_url,
                kind,
            }
        }
        RawContext::StatusContext {
            context,
            state,
            description,
            target_url,
            created_at,
        } => {
            let state = status_state(&state);
            Check {
                name: context,
                workflow: None,
                state,
                started_at: created_at,
                completed_at: None,
                url: target_url,
                kind: CheckKind::Status { description },
            }
        }
    }
}

/// Same buckets as `gh pr checks`.
fn check_run_state(status: &str, conclusion: Option<&str>) -> CheckState {
    if status != "COMPLETED" {
        return CheckState::Pending;
    }
    match conclusion {
        Some("SUCCESS") => CheckState::Pass,
        Some("SKIPPED" | "NEUTRAL") => CheckState::Skipped,
        Some("FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE") => {
            CheckState::Fail
        }
        _ => CheckState::Pending,
    }
}

fn status_state(state: &str) -> CheckState {
    match state {
        "SUCCESS" => CheckState::Pass,
        "FAILURE" | "ERROR" => CheckState::Fail,
        _ => CheckState::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Rollup;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_recorded_prs_with_a_missing_one() {
        let prs = prs(&fixture("prs.json"), &[69, 99999]).unwrap();
        let pr = prs[0].1.as_ref().unwrap();
        assert_eq!(pr.number, 69);
        assert_eq!(pr.state, PrState::Merged);
        assert_eq!(pr.author, "fpacifici");
        assert_eq!(pr.head, "tui-layout");
        assert_eq!(pr.url, "https://github.com/fpacifici/assistant/pull/69");
        assert_eq!(pr.rollup(), Rollup::Success);
        assert_eq!(pr.checks.len(), 2);
        let ci = &pr.checks[0];
        assert_eq!(ci.name, "ci");
        assert_eq!(ci.workflow.as_deref(), Some("CI"));
        assert_eq!(
            ci.kind,
            CheckKind::ActionsJob {
                job_id: 114153140536
            }
        );
        assert!(ci.completed_at.unwrap() > ci.started_at.unwrap());

        assert_eq!(prs[1].0, 99999);
        assert_eq!(prs[1].1.as_ref().unwrap_err(), "not found");
    }

    #[test]
    fn missing_repo_is_not_found() {
        let err = prs(&fixture("repo_missing.json"), &[69, 99999]).unwrap_err();
        assert!(matches!(err, ApiError::NotFound(m) if m.contains("does-not-exist")));
    }

    #[test]
    fn rate_limit_is_detected() {
        let response = json!({"data": null, "errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
        assert_eq!(
            prs(&response, &[1]).unwrap_err(),
            ApiError::RateLimited { reset: None }
        );
    }

    #[test]
    fn parses_open_prs() {
        let prs = open_prs(&fixture("open_prs.json")).unwrap();
        assert_eq!(prs.len(), 3);
        assert_eq!(prs[0].number, 2825);
        assert_eq!(prs[0].author, "joshka");
    }

    fn pr_with(contexts: Value) -> Value {
        json!({"data": {"repository": {"pr_1": {
            "number": 1, "title": "t", "body": "b", "url": "u", "state": "OPEN",
            "author": null, "headRefName": "h", "baseRefName": "main",
            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": contexts}}}}]}
        }}}})
    }

    fn run(name: &str, status: &str, conclusion: Option<&str>) -> Value {
        json!({"__typename": "CheckRun", "databaseId": 5, "name": name, "status": status,
               "conclusion": conclusion, "startedAt": null, "completedAt": null,
               "detailsUrl": null, "title": "T", "summary": "S",
               "checkSuite": {"app": {"slug": "some-app"}, "workflowRun": null}})
    }

    #[test]
    fn checks_are_bucketed_and_sorted_like_gh() {
        let response = pr_with(json!([
            run("e-pass", "COMPLETED", Some("SUCCESS")),
            run("d-skip", "COMPLETED", Some("SKIPPED")),
            run("c-run", "IN_PROGRESS", None),
            run("b-cancel", "COMPLETED", Some("CANCELLED")),
            {"__typename": "StatusContext", "context": "a-ext", "state": "ERROR",
             "description": "boom", "targetUrl": "https://ci", "createdAt": null},
        ]));
        let pr = prs(&response, &[1]).unwrap().remove(0).1.unwrap();
        let got: Vec<_> = pr
            .checks
            .iter()
            .map(|c| (c.name.as_str(), c.state))
            .collect();
        assert_eq!(
            got,
            [
                ("a-ext", CheckState::Fail),
                ("b-cancel", CheckState::Fail),
                ("c-run", CheckState::Pending),
                ("e-pass", CheckState::Pass),
                ("d-skip", CheckState::Skipped),
            ]
        );
        assert_eq!(pr.rollup(), Rollup::Failure);
        assert!(pr.checks_running());
        assert_eq!(pr.author, "ghost");
        assert_eq!(
            pr.checks[0].kind,
            CheckKind::Status {
                description: Some("boom".into())
            }
        );
        assert!(matches!(pr.checks[2].kind, CheckKind::CheckRun { .. }));
    }

    #[test]
    fn pr_without_checks_has_no_rollup() {
        let response = pr_with(json!([]));
        let pr = prs(&response, &[1]).unwrap().remove(0).1.unwrap();
        assert_eq!(pr.rollup(), Rollup::None);
    }
}
