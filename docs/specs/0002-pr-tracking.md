# 0002 — PR tracking

Status: Accepted

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
  (`d`) from the UI, which rewrites the file (comments are not preserved).

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
   failure, yellow pending, gray none). Merged/closed PRs are tagged
   `[merged]` / `[closed]` and stay until untracked.
3. **Detail** (right):
   - Top third: a header line with author, `head → base`, state and the PR
     URL, followed by the raw markdown description, truncated to fit.
   - Below: checks in the style of `gh pr checks`: a summary line
     (`2 failing, 1 pending, 10 successful`), then one row per check with a
     status glyph (`✓` pass, `X` fail — including cancelled and timed out,
     `*` pending, `-` skipped/neutral; same buckets as `gh pr checks`), name, workflow or context, and elapsed time. Failing first,
     then pending, then passing/other.
4. **Status bar** — transient errors (failed poll, config write failure,
   rate limiting). Cleared by the next successful poll.
5. **Footer** — key bindings.

Checks are those of the PR's head commit: check runs and legacy commit
statuses.

### Polling

- Opening a repo tab for the first time fetches every tracked PR in it.
- The selected PR is refreshed every 5 s while any check is pending, and
  every 60 s once all checks have completed.
- `r` refreshes the selected PR (and the open log) immediately.
- When rate limited, polling pauses until the reset time and the status bar
  says so.

### Add-PR picker (`Ctrl+A`)

A modal listing the 50 most recently updated open PRs of the current repo.
Already tracked PRs are marked `✓`. `Enter` on an untracked PR adds it to the
config, closes the picker and selects the new PR; `Enter` on a tracked PR
does nothing.

### Log view (`Enter` on a check)

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
| `←` / `→`, `h` / `l`             | Focus PR list / checks                              |
| `↑` / `↓`, `k` / `j`             | Move within the focused pane / scroll the log       |
| `PgUp` / `PgDn`, `g` / `G`       | Page / jump to top or bottom (log view)             |
| `Enter`                          | Open log view (checks) / add PR (picker)            |
| `Ctrl+A`                         | Open the add-PR picker                              |
| `d`                              | Untrack the selected PR                             |
| `r`                              | Refresh now                                         |
| `y`                              | Copy the PR URL to the clipboard (OSC 52)           |
| `o`                              | Open the PR (or the focused check) in the browser   |
| `Esc`                            | Close the modal                                     |
| `q`, `Ctrl+C`                    | Quit (`q` closes the modal when one is open)        |

## Acceptance criteria

- [ ] Config is loaded from `~/.ghui/config.yaml`, created if missing, and
      rewritten when PRs are added or removed.
- [ ] Auth falls back GH_TOKEN → GITHUB_TOKEN → `gh auth token`, and exits
      with a clear error otherwise.
- [ ] One tab per repo; inaccessible repos are dimmed and show their error.
- [ ] PR list shows aggregate status, number, title and merged/closed tags.
- [ ] Detail shows header with URL, raw description (top third) and checks.
- [ ] The selected PR's checks refresh every 5 s while pending, 60 s after.
- [ ] Picker lists 50 open PRs, marks tracked ones and adds on Enter.
- [ ] `d` untracks the selected PR.
- [ ] Log view shows cleaned Actions logs, follows running jobs, and shows a
      summary for non-Actions checks.
- [ ] `y` copies and `o` opens the URL.
- [ ] State transitions, scheduling, parsing and rendering are unit tested
      without network access.

## Out of scope

- Adding or removing repos from the UI.
- Filtering/searching the picker; more than 50 PRs.
- Rendering markdown; scrolling the description.
- Re-running checks, commenting, reviewing or merging.
- Mouse support.
