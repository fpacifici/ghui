# 0007 — Workflows, jobs and steps in the checks pane

Status: Implemented

Extends [0002-pr-tracking.md](0002-pr-tracking.md). The API has no live log
for a running Actions job, so while a job runs the log window can only say
"logs appear when it completes". The job's steps, which the API does report
live, show how far it has got and which step failed.

## Goal

The checks pane is a tree: Actions workflow runs, their jobs, and each job's
steps. What is running is expanded by default, so progress shows without any
key presses.

## Behaviour

### Tree

```
1 failing, 1 pending, 3 successful, 0 skipped

▾ * CI                          2 jobs   1m10s
    ▸ X lint                             20s
    ▾ * test                             1m10s
        ✓ Set up job                     1s
        * Run tests                      1m8s
          * Post checkout
▸ ✓ Release                     2 jobs   3m2s
  ✓ deploy/preview                       12s
```

- **Workflow runs** are top-level rows: the workflow's name, how many jobs
  it has, its state (the worst of its jobs: failing, then pending, then
  passing, then skipped) and its elapsed time (from its first job's start
  to its last job's end).
- **Jobs** (Actions check runs) are listed under their run, in the order of
  0002 (failing, pending, passing, skipped, then by name).
- **Steps** are listed under their job in execution order, with state and
  elapsed time. Steps that have not started show no time.
- Checks that are not Actions jobs (other apps' check runs, commit
  statuses) stay top-level leaves.
- Top-level rows are ordered as checks were in 0002, by their state and
  then name.
- The summary line still counts checks (jobs), not runs or steps.

### Expansion

- By default a workflow run is expanded while any of its jobs is pending,
  and a job is expanded while it is pending. Everything else is collapsed.
- Toggling a row by hand overrides the default for that run or job for the
  rest of the session; it is never reset by polls.
- Expansion and the selection live per repo tab, like the selected check
  in 0002. The selection is a row: as rows appear and disappear it stays at
  the same position, clamped.

### Steps are fetched for the selected PR only

Steps make the checks query much more expensive, so they are only requested
for the active tab's selected PR. Selecting another PR refreshes the tab right
away, so its steps appear without waiting for the next poll. Jobs whose steps
are not loaded have no expand marker.

### Opening rows

- `Enter` on a workflow run toggles it.
- `Enter` on a job behaves as `Enter` on a check did (0002, 0005): running
  jobs open in the browser, others open the log window.
- `Enter` on a step does the same for its job, opening the browser at that
  step.
- `o` and `y` on a step use the job's URL at that step.

## Key bindings

In the checks pane:

| Key                  | Action                                                      |
|----------------------|-------------------------------------------------------------|
| `Space`              | Expand / collapse the run or job; on a step, collapse its job |
| `→`, `l`             | Expand the run or job                                       |
| `←`, `h`             | Collapse the run or job; on anything else go to its parent, or to the PR list from the top level |
| `Enter`              | Toggle a run; open a job or step as above                   |

## Acceptance criteria

- [x] Actions jobs are grouped under their workflow run; other checks are
      top-level.
- [x] Runs show their job count, aggregate state and elapsed time.
- [x] Steps are shown under their job with state and elapsed time.
- [x] Pending runs and jobs are expanded by default; manual toggles stick.
- [x] Steps are only requested for the active tab's selected PR, and
      selecting another PR refreshes the tab.
- [x] Keys work as in the table above.
- [x] Parsing, tree building, key handling and rendering are unit tested
      without network access.

## Out of scope

- Live logs for running steps (the API does not provide them).
- Jumping to a step in the log window.
- Re-running jobs.
