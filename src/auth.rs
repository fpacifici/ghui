//! Resolves the GitHub token: `GH_TOKEN`, then `GITHUB_TOKEN`, then
//! `gh auth token`.

use std::process::Command;

pub const MISSING_TOKEN: &str = "no GitHub token found: set GH_TOKEN (or GITHUB_TOKEN), \
     or run `gh auth login` so that `gh auth token` works";

/// Resolves the token from the real environment and `gh` CLI.
pub fn token() -> Option<String> {
    resolve(|name| std::env::var(name).ok(), gh_auth_token)
}

/// Resolution order, with the lookups injected so it can be tested.
pub fn resolve(
    env: impl Fn(&str) -> Option<String>,
    gh: impl FnOnce() -> Option<String>,
) -> Option<String> {
    ["GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .find_map(|name| non_empty(env(name)))
        .or_else(|| non_empty(gh()))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn gh_auth_token() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn gh_token_wins() {
        let vars = [("GH_TOKEN", "a"), ("GITHUB_TOKEN", "b")];
        assert_eq!(resolve(env(&vars), || Some("c".into())), Some("a".into()));
    }

    #[test]
    fn falls_back_to_github_token_then_gh_cli() {
        let vars = [("GH_TOKEN", "  "), ("GITHUB_TOKEN", "b")];
        assert_eq!(resolve(env(&vars), || None), Some("b".into()));
        assert_eq!(resolve(env(&[]), || Some("c\n".into())), Some("c".into()));
    }

    #[test]
    fn none_when_nothing_is_available() {
        assert_eq!(resolve(env(&[]), || None), None);
        assert_eq!(resolve(env(&[]), || Some(String::new())), None);
    }
}
