//! GraphQL documents. PRs are fetched with one alias per number
//! (`pr_123: pullRequest(number: 123)`) so a whole repo is one request.

const PR_FRAGMENT: &str = "fragment Pr on PullRequest {
  number title body url state
  author { login }
  headRefName baseRefName
  commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
    __typename
    ... on CheckRun {
      databaseId name status conclusion startedAt completedAt detailsUrl title summary
      checkSuite { app { slug } workflowRun { workflow { name } } }
    }
    ... on StatusContext { context state description targetUrl createdAt }
  } } } } } }
}";

/// The alias under which PR `number` appears in the response.
pub fn pr_alias(number: u64) -> String {
    format!("pr_{number}")
}

/// Fetches the given PRs of `$owner/$name`. With no numbers it still
/// resolves the repository, which doubles as an access check.
pub fn prs(numbers: &[u64]) -> String {
    let fields: String = numbers
        .iter()
        .map(|n| format!(" {}: pullRequest(number: {n}) {{ ...Pr }}", pr_alias(*n)))
        .collect();
    // GraphQL rejects unused fragments, so only include it when needed.
    let fragment = if numbers.is_empty() { "" } else { PR_FRAGMENT };
    format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ nameWithOwner{fields} }} }}\n{fragment}"
    )
}

/// The 50 most recently updated open PRs of `$owner/$name`.
pub const OPEN_PRS: &str = "query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(states: OPEN, first: 50, orderBy: { field: UPDATED_AT, direction: DESC }) {
      nodes { number title isDraft updatedAt author { login } }
    }
  }
}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prs_query_aliases_every_number() {
        let q = prs(&[1, 42]);
        assert!(q.contains("pr_1: pullRequest(number: 1) { ...Pr }"));
        assert!(q.contains("pr_42: pullRequest(number: 42) { ...Pr }"));
        assert!(q.contains("fragment Pr on PullRequest"));
    }

    #[test]
    fn prs_query_without_numbers_only_resolves_the_repo() {
        let q = prs(&[]);
        assert!(!q.contains("pullRequest("));
        assert!(!q.contains("fragment"));
    }
}
