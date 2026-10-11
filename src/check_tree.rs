//! The checks pane as a tree: Actions workflow runs, their jobs and the
//! jobs' steps (spec 0007). Pure: builds the visible rows of a PR from its
//! checks and the user's expand/collapse overrides.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::github::{Check, CheckState, PrDetail, Step};

/// A row that can be expanded or collapsed. Run and job ids are global, so
/// overrides can be kept per repo without the PR number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fold {
    Run(u64),
    Job(u64),
}

/// Manual expand (`true`) / collapse (`false`) overrides.
pub type Folds = HashMap<Fold, bool>;

/// A workflow run and its jobs, in display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<'a> {
    pub id: u64,
    pub name: &'a str,
    pub jobs: Vec<&'a Check>,
}

impl Run<'_> {
    /// The worst state of its jobs: failing, pending, passing, skipped.
    pub fn state(&self) -> CheckState {
        self.jobs
            .iter()
            .map(|j| j.state)
            .min()
            .unwrap_or(CheckState::Skipped)
    }

    /// Whether any job is pending; a run can be failing and running.
    pub fn is_running(&self) -> bool {
        self.jobs.iter().any(|j| j.is_running())
    }

    pub fn started_at(&self) -> Option<DateTime<Utc>> {
        self.jobs.iter().filter_map(|j| j.started_at).min()
    }

    /// When its last job completed; `None` while any job runs.
    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        if self.is_running() {
            return None;
        }
        self.jobs.iter().filter_map(|j| j.completed_at).max()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'a> {
    Run(Run<'a>),
    /// An Actions job, or a check that is not one.
    Check(&'a Check),
    Step {
        job: &'a Check,
        step: &'a Step,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row<'a> {
    pub depth: usize,
    pub node: Node<'a>,
    /// The fold and whether it is expanded, for rows that have children.
    pub fold: Option<(Fold, bool)>,
}

impl<'a> Row<'a> {
    /// The check a row stands for: the job itself, or a step's job.
    pub fn check(&self) -> Option<&'a Check> {
        match self.node {
            Node::Run(_) => None,
            Node::Check(check) | Node::Step { job: check, .. } => Some(check),
        }
    }

    /// Where the row opens in the browser: a run's page, a job's, or a
    /// job's page at a step.
    pub fn url(&self) -> Option<String> {
        match &self.node {
            // Job URLs look like `…/actions/runs/{run}/job/{job}`.
            Node::Run(run) => {
                let url = run.jobs.first()?.url.as_deref()?;
                Some(url.split_once("/job/").map_or(url, |(run, _)| run).into())
            }
            Node::Check(check) => check.url.clone(),
            Node::Step { job, step } => job.step_url(step.number),
        }
    }
}

/// The visible rows of `detail`'s checks.
pub fn rows<'a>(detail: &'a PrDetail, folds: &Folds) -> Vec<Row<'a>> {
    let mut rows = Vec::new();
    for node in top_level(&detail.checks) {
        match node {
            Node::Run(run) => {
                let fold = Fold::Run(run.id);
                let expanded = folds.get(&fold).copied().unwrap_or(run.is_running());
                let jobs = run.jobs.clone();
                rows.push(Row {
                    depth: 0,
                    node: Node::Run(run),
                    fold: Some((fold, expanded)),
                });
                if expanded {
                    for job in jobs {
                        push_check(&mut rows, job, 1, folds);
                    }
                }
            }
            Node::Check(check) => push_check(&mut rows, check, 0, folds),
            Node::Step { .. } => unreachable!("steps are never top-level"),
        }
    }
    rows
}

/// Runs and checks outside any run. `checks` are sorted by state then
/// name, so ordering runs by their first job sorts them the same way.
fn top_level(checks: &[Check]) -> Vec<Node<'_>> {
    let mut nodes: Vec<Node> = Vec::new();
    for check in checks {
        let Some(id) = check.run_id.filter(|_| check.job_id().is_some()) else {
            nodes.push(Node::Check(check));
            continue;
        };
        let existing = nodes.iter_mut().find_map(|n| match n {
            Node::Run(run) if run.id == id => Some(run),
            _ => None,
        });
        match existing {
            Some(run) => run.jobs.push(check),
            None => nodes.push(Node::Run(Run {
                id,
                name: check.workflow.as_deref().unwrap_or("workflow"),
                jobs: vec![check],
            })),
        }
    }
    nodes
}

fn push_check<'a>(rows: &mut Vec<Row<'a>>, check: &'a Check, depth: usize, folds: &Folds) {
    let fold = check
        .job_id()
        .filter(|_| !check.steps.is_empty())
        .map(|id| {
            let fold = Fold::Job(id);
            let expanded = folds.get(&fold).copied().unwrap_or(check.is_running());
            (fold, expanded)
        });
    rows.push(Row {
        depth,
        node: Node::Check(check),
        fold,
    });
    if fold.is_some_and(|(_, expanded)| expanded) {
        rows.extend(check.steps.iter().map(|step| Row {
            depth: depth + 1,
            node: Node::Step { job: check, step },
            fold: None,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{check, detail, job_id, t};
    use crate::github::CheckKind;

    fn job(name: &str, run: u64, state: CheckState, steps: &[CheckState]) -> Check {
        let steps = steps
            .iter()
            .enumerate()
            .map(|(i, state)| Step {
                number: i as u64 + 1,
                name: format!("{name} step {}", i + 1),
                state: *state,
                started_at: Some(t(0)),
                completed_at: None,
            })
            .collect();
        Check {
            run_id: Some(run),
            steps,
            ..check(name, state)
        }
    }

    fn status(name: &str, state: CheckState) -> Check {
        Check {
            workflow: None,
            kind: CheckKind::Status { description: None },
            ..check(name, state)
        }
    }

    /// `depth name [+/-]` per row; `+` expanded, `-` collapsed.
    fn outline(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let name = match &row.node {
                    Node::Run(run) => format!("{} ({})", run.name, run.jobs.len()),
                    Node::Check(check) => check.name.clone(),
                    Node::Step { step, .. } => step.name.clone(),
                };
                let fold = match row.fold {
                    Some((_, true)) => " +",
                    Some((_, false)) => " -",
                    None => "",
                };
                format!("{}{name}{fold}", "  ".repeat(row.depth))
            })
            .collect()
    }

    fn sample() -> PrDetail {
        // Sorted as the parser sorts: by state, then name.
        let mut release = job("pkg", 2, CheckState::Pass, &[CheckState::Pass]);
        release.workflow = Some("Release".into());
        detail(
            1,
            vec![
                status("ext", CheckState::Fail),
                job("lint", 1, CheckState::Fail, &[CheckState::Fail]),
                job(
                    "test",
                    1,
                    CheckState::Pending,
                    &[CheckState::Pass, CheckState::Pending],
                ),
                release,
            ],
        )
    }

    #[test]
    fn running_runs_and_jobs_are_expanded_by_default() {
        let detail = sample();
        assert_eq!(
            outline(&rows(&detail, &Folds::new())),
            [
                "ext",
                "CI (2) +",
                "  lint -",
                "  test +",
                "    test step 1",
                "    test step 2",
                "Release (1) -",
            ]
        );
    }

    #[test]
    fn overrides_win_over_the_defaults() {
        let detail = sample();
        let folds = Folds::from([
            (Fold::Job(job_id("test")), false),
            (Fold::Job(job_id("lint")), true),
            (Fold::Run(2), true),
        ]);
        assert_eq!(
            outline(&rows(&detail, &folds)),
            [
                "ext",
                "CI (2) +",
                "  lint +",
                "    lint step 1",
                "  test -",
                "Release (1) +",
                "  pkg -",
            ]
        );
        let folds = Folds::from([(Fold::Run(1), false)]);
        assert_eq!(
            outline(&rows(&detail, &folds)),
            ["ext", "CI (2) -", "Release (1) -"]
        );
    }

    #[test]
    fn jobs_without_steps_do_not_fold() {
        let detail = detail(1, vec![job("build", 1, CheckState::Pending, &[])]);
        assert_eq!(
            outline(&rows(&detail, &Folds::new())),
            ["CI (1) +", "  build"]
        );
    }

    #[test]
    fn runs_aggregate_state_and_times() {
        let mut a = job("a", 1, CheckState::Pass, &[]);
        a.started_at = Some(t(10));
        a.completed_at = Some(t(50));
        let mut b = job("b", 1, CheckState::Skipped, &[]);
        b.started_at = Some(t(5));
        b.completed_at = Some(t(70));
        let run = Run {
            id: 1,
            name: "CI",
            jobs: vec![&a, &b],
        };
        assert_eq!(run.state(), CheckState::Pass);
        assert_eq!(run.started_at(), Some(t(5)));
        assert_eq!(run.completed_at(), Some(t(70)));

        let c = job("c", 1, CheckState::Pending, &[]);
        let run = Run {
            jobs: vec![&a, &c],
            ..run
        };
        assert_eq!(run.state(), CheckState::Pending);
        assert_eq!(run.completed_at(), None);

        // Failing wins the state, but the run is still running.
        let d = job("d", 1, CheckState::Fail, &[]);
        let run = Run {
            jobs: vec![&d, &c],
            ..run
        };
        assert_eq!(run.state(), CheckState::Fail);
        assert!(run.is_running());
        assert_eq!(run.completed_at(), None);
    }

    #[test]
    fn row_check_is_the_job_for_steps() {
        let detail = sample();
        let rows = rows(&detail, &Folds::new());
        assert_eq!(rows[1].check(), None);
        assert_eq!(rows[4].check().unwrap().name, "test");
    }
}
