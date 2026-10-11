//! GraphQL documents. PRs are fetched with one alias per number
//! (`pr_123: pullRequest(number: 123)`) so a whole repo is one request.

/// The PR fields; `STEPS` marks where job steps go when requested.
const PR_FRAGMENT: &str = "fragment NAME on PullRequest {
  number title body url state
  author { login }
  headRefName baseRefName
  commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
    __typename
    ... on CheckRun {
      databaseId name status conclusion startedAt completedAt detailsUrl title summary
      checkSuite { app { slug } workflowRun { databaseId workflow { name } } }STEPS
    }
    ... on StatusContext { context state description targetUrl createdAt }
  } } } } } }
}";

/// The alias under which PR `number` appears in the response.
pub fn pr_alias(number: u64) -> String {
    format!("pr_{number}")
}

const STEPS: &str =
    "\n      steps(first: 100) { nodes { number name status conclusion startedAt completedAt } }";

/// Fetches the given PRs of `$owner/$name`, with job steps for PR `steps`
/// only: steps under every check of every PR would multiply the query's
/// rate-limit cost. With no numbers it still resolves the repository, which
/// doubles as an access check.
pub fn prs(numbers: &[u64], steps: Option<u64>) -> String {
    let with_steps = |n: u64| steps == Some(n);
    let fields: String = numbers
        .iter()
        .map(|&n| {
            let fragment = if with_steps(n) { "PrSteps" } else { "Pr" };
            format!(
                " {}: pullRequest(number: {n}) {{ ...{fragment} }}",
                pr_alias(n)
            )
        })
        .collect();
    // GraphQL rejects unused fragments, so only include those in use.
    let mut fragments = String::new();
    if numbers.iter().any(|&n| !with_steps(n)) {
        fragments.push_str(&PR_FRAGMENT.replace("NAME", "Pr").replace("STEPS", ""));
        fragments.push('\n');
    }
    if numbers.iter().any(|&n| with_steps(n)) {
        fragments.push_str(
            &PR_FRAGMENT
                .replace("NAME", "PrSteps")
                .replace("STEPS", STEPS),
        );
        fragments.push('\n');
    }
    format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ nameWithOwner{fields} }} }}\n{fragments}"
    )
}

/// The 50 most recently updated open PRs of `$owner/$name`, and the
/// first 50 results of the search `$mine` (see [`mine_search`]).
pub const OPEN_PRS: &str = "query($owner: String!, $name: String!, $mine: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: 50, orderBy: { field: UPDATED_AT, direction: DESC }) {
      nodes { ...Summary }
    }
  }
  mine: search(query: $mine, type: ISSUE, first: 50) {
    nodes { ...Summary }
  }
}
fragment Summary on PullRequest { number title isDraft updatedAt author { login } }";

/// Search for the authenticated user's open PRs in `repo`.
pub fn mine_search(repo: &str) -> String {
    format!("repo:{repo} is:pr is:open author:@me sort:updated-desc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prs_query_aliases_every_number() {
        let q = prs(&[1, 42], None);
        assert!(q.contains("pr_1: pullRequest(number: 1) { ...Pr }"));
        assert!(q.contains("pr_42: pullRequest(number: 42) { ...Pr }"));
        assert!(q.contains("fragment Pr on PullRequest"));
        assert!(!q.contains("PrSteps"));
        assert!(!q.contains("steps("));
    }

    #[test]
    fn prs_query_requests_steps_for_one_pr_only() {
        let q = prs(&[1, 42], Some(42));
        assert!(q.contains("pr_1: pullRequest(number: 1) { ...Pr }"));
        assert!(q.contains("pr_42: pullRequest(number: 42) { ...PrSteps }"));
        assert!(q.contains("fragment Pr on PullRequest"));
        assert!(q.contains("fragment PrSteps on PullRequest"));
        assert_eq!(q.matches("steps(first: 100)").count(), 1);

        let q = prs(&[42], Some(42));
        assert!(!q.contains("fragment Pr on"), "unused fragment: {q}");
    }

    #[test]
    fn prs_query_without_numbers_only_resolves_the_repo() {
        let q = prs(&[], Some(1));
        assert!(!q.contains("pullRequest("));
        assert!(!q.contains("fragment"));
    }
}
