# 0002 — PR tracking

Status: Implemented

Supersedes [0001-basic-ui.md](0001-basic-ui.md). ghui is not a general
GitHub client: it tracks a hand-picked set of pull requests across repos and
watches their checks.

## Goal

Track specific PRs in specific repos, see their description and checks at a
glance, watch checks update live (like `gh pr checks --watch`) and read job
logs without leaving the terminal.

## Configuration

`~/.ghui/config.yaml`:

```yaml
repos:
  - name: owner/repo
    prs: [123, 456]
  - name: owner/other
    prs: []
```

- The list order is the tab order.
- If the file is missing it is created empty and the UI shows a hint.
- Repos are added by editing the file. PRs are added (`Ctrl+A`) and removed
  (`Ctrl+D` or `d`) from the UI, which rewrites the file (comments are not
  preserved).

## Authentication

The token is taken from `GH_TOKEN`, then `GITHUB_TOKEN`, then the output of
`gh auth token`. If none yields a token, ghui exits before starting the TUI
with a message telling the user to set `GH_TOKEN` or run `gh auth login`.

## Behaviour

### Layout

1. **Repo tabs** — one tab per configured repo. A repo that cannot be loaded
   (not found, no access, network error) is dimmed; selecting it shows the
   error in the body. Network errors are retried on the next poll.
2. **PR list** (left, narrow) — one row per tracked PR:
   `● #123 Title…`. The dot is the aggregate check state (green success, red
   failure, yellow pending, gray none). Merged/closed PRs stay until
   untracked and are tagged `merged` / `closed` (see
   [0008-manual-tracking.md](0008-manual-tracking.md)).
3. **Detail** (right):
   - Top third: a header line with author, `head → base`, state and the PR
     URL, followed by the raw markdown description, truncated to fit.
   - Below: checks in the style of `gh pr checks`: a summary line
     (`2 failing, 1 pending, 10 successful`), then one row per check with a
     status glyph (`✓` pass, `X` fail — including cancelled and timed out,
     `*` pending, `-` skipped/neutral; same buckets as `gh pr checks`), name, workflow or context, and elapsed time. Failing first,
     then pending, then passing/other. Actions jobs are grouped under
     their workflow run, with their steps; see
     [0007-check-tree.md](0007-check-tree.md).
4. **Status bar** — transient errors (failed poll, config write failure,
   rate limiting). Cleared by the next successful poll.
5. **Footer** — key bindings.

Checks are those of the PR's head commit: check runs and legacy commit
statuses.

### Polling

Replaced by [0008-manual-tracking.md](0008-manual-tracking.md): every tab
is fetched at start-up, then only PRs with pending checks are polled,
every 30 s, in every tab. `r` refreshes every tab (in the log window it
refreshes the log). When rate limited, polling pauses until the reset time
and the status bar says so.

### Add-PR picker (`Ctrl+A`)

A modal listing open PRs of the current repo: the user's, or the 50 most
recently updated, filterable by number; see
[0008-manual-tracking.md](0008-manual-tracking.md). Already tracked PRs are
marked `✓`. `Enter` on an untracked PR adds it to the
config, closes the picker and selects the new PR; `Enter` on a tracked PR
does nothing.

### Untracking a PR (`Ctrl+D` / `d`)

Removes the selected PR of the current repo from the config and from the PR
list, with no confirmation (it can be re-added with `Ctrl+A`). The selection
stays at the same row, clamped to the shortened list, and moves to the first
check. Results of a poll that was in flight for the PR are discarded. The key
does nothing when no PR is selected or while a modal (picker, log window) is
open.

### Log view (`Enter` on a check)

`Enter` on a running check opens it in the browser instead
([spec 0005](0005-enter-opens-browser.md)); the log view opens for
completed checks and for running checks without a URL.

- Full-screen overlay titled with the check name and state.
- For GitHub Actions jobs, the job log is shown with the per-line timestamp
  and ANSI escape codes removed. A failed job opens at the first `##[error]`
  line; otherwise at the bottom.
- While the job is running the log is re-fetched every 5 s and the view
  follows the end unless the user has scrolled up. The public API exposes no
  live log stream; if no log is available yet the view says
  "Job in progress — logs appear when it completes".
- For checks that are not Actions jobs (third-party check runs, commit
  statuses) the view shows the check's title/summary or description and its
  URL.

## Key bindings

| Key                              | Action                                              |
|----------------------------------|-----------------------------------------------------|
| `<` / `>`, `Shift+Tab` / `Tab`   | Previous / next repo tab                            |
| `←` / `→`, `h` / `l`             | Focus PR list / checks; fold in the checks tree (0007) |
| `Space`                          | Expand / collapse in the checks tree (0007)         |
| `↑` / `↓`, `k` / `j`             | Move within the focused pane / scroll the log       |
| `PgUp` / `PgDn`, `g` / `G`       | Page / jump to top or bottom (log view)             |
| `Enter`                          | Open PR in browser (list) / running check in browser, else log view (checks) / add PR (picker) |
| `Ctrl+A`                         | Open the add-PR picker                              |
| `Ctrl+D`, `d`                    | Untrack the selected PR                             |
| `r`                              | Refresh every tab (0006) / the log                  |
| `y`                              | Copy the PR URL to the clipboard (OSC 52)           |
| `o`                              | Open the PR (or the focused check) in the browser   |
| `Esc`                            | Close the modal                                     |
| `q`, `Ctrl+C`                    | Quit (`q` closes the modal when one is open)        |

## Acceptance criteria

- [x] Config is loaded from `~/.ghui/config.yaml`, created if missing, and
      rewritten when PRs are added or removed.
- [x] Auth falls back GH_TOKEN → GITHUB_TOKEN → `gh auth token`, and exits
      with a clear error otherwise.
- [x] One tab per repo; inaccessible repos are dimmed and show their error.
- [x] PR list shows aggregate status, number and title.
- [x] Detail shows header with URL, raw description (top third) and checks.
- [x] The selected PR's checks refresh every 5 s while pending, 60 s after
      (now every 30 s while pending only; see 0008).
- [x] Picker lists 50 open PRs, marks tracked ones and adds on Enter.
- [x] `Ctrl+D` (or `d`) untracks the selected PR and persists the change;
      it is a no-op while a modal is open.
- [x] Log view shows cleaned Actions logs, follows running jobs, and shows a
      summary for non-Actions checks.
- [x] `y` copies and `o` opens the URL.
- [x] State transitions, scheduling, parsing and rendering are unit tested
      without network access.

## Out of scope

- Adding or removing repos from the UI.
- Searching the picker by title; more than 50 PRs (number filter: 0008).
- Rendering markdown; scrolling the description.
- Re-running checks, commenting, reviewing or merging.
- Mouse support.
