# Architecture

ghui is a single-binary Rust application that renders a terminal UI with
[ratatui](https://ratatui.rs) on top of the crossterm backend. It tracks a
configured set of GitHub PRs and watches their checks.

## Overview

```
                 Action                         Effect
terminal ─▶ event::map_key ─▶ App::apply ─────────┬──▶ worker (tokio) ──▶ GitHub API
 input                          App::tick (timer) ─┤          │
                                                   │          │ Msg (mpsc)
                                                   │          ▼
                                                   │     App::handle ──▶ Effect
                                                   └──▶ main: save config,
                                                        OSC 52 copy, open URL
terminal ◀── ui::render(&App)
 output
```

The main loop in `main.rs` is synchronous and repeats:

1. **Draw** — `ui::render(&app, frame)`.
2. **Input** — `event::next_action` waits up to 100 ms for a key and maps it
   to an `Action`; `App::apply` returns `Effect`s.
3. **Results** — every `Msg` waiting on the channel goes to `App::handle`,
   which may return `Effect`s too (e.g. check notifications).
4. **Timers** — `App::tick(now)` returns the polls that are due.
5. **Effects** — network effects are spawned on the tokio runtime by
   `worker::Worker`, which reports each completed request as a
   `Msg::ApiCall` (stamped with when it was sent) before its result, for
   the API log ([spec 0009](specs/0009-api-log.md)); the rest (config save, clipboard, browser) run inline,
   except desktop notifications, which get a thread of their own.

`App` never performs I/O. Everything it wants done is an `Effect`, and
everything that happened is an `Action` or a `Msg`, so all behaviour —
including the poll schedule, driven by an explicit `now` — is unit tested
without a terminal, network or real clock.

## Modules

| Module            | Responsibility                                                           | Side effects        |
|-------------------|--------------------------------------------------------------------------|---------------------|
| `main.rs`         | Auth + config before the TUI, tokio runtime, main loop, local effects.   | Terminal, files, stdout, desktop notifications |
| `auth.rs`         | Token from `GH_TOKEN` → `GITHUB_TOKEN` → `gh auth token`.                | Runs `gh`           |
| `config.rs`       | `~/.ghui/config.yaml`: load, create if missing, atomic save, track/untrack. | Files (load/save only) |
| `event.rs`        | `Action` enum, context-free `map_key`.                                    | Reads stdin events  |
| `app.rs`          | `App` state (repos, focus, modal, status), `apply` / `handle` / `tick`.  | None                |
| `check_tree.rs`   | Checks pane rows: workflow runs → jobs → steps, folds.                   | None                |
| `log_view.rs`     | Log window state: content, scroll/follow, log poll bookkeeping.          | None                |
| `scroll.rs`       | `Scroll`: top line and follow-the-end state shared by the log windows.   | None                |
| `api_log.rs`      | API call log: `Request::of(&Effect)`, `ApiCall`, last 1000 calls.        | None                |
| `notify.rs`       | Desktop notifications: `osascript` on macOS, `notify-rust` elsewhere.    | Runs `osascript` / D-Bus |
| `logs.rs`         | Cleans raw Actions logs (BOM, timestamps, ANSI, `\r`, endgroup).         | None                |
| `worker.rs`       | Turns network `Effect`s into tokio tasks that send back a `Msg`, preceded by a `Msg::ApiCall`. | Spawns tasks        |
| `github/mod.rs`   | `GitHub` trait and domain types (`PrDetail`, `Check`, `ApiError`, …).    | None                |
| `github/query.rs` | GraphQL documents.                                                       | None                |
| `github/parse.rs` | GraphQL JSON → domain types; tested against `tests/fixtures/`.           | None                |
| `github/octo.rs`  | `GitHub` implemented with octocrab; HTTP error mapping.                  | Network             |
| `ui/`             | Pure rendering: tabs, PR list, detail, checks, picker, log windows.      | None                |

## GitHub access

- **PRs and checks**: one GraphQL request per repo, with one alias per
  tracked PR (`pr_123: pullRequest(number: 123)`). Each PR includes its
  head commit's `statusCheckRollup`, which carries both check runs and
  legacy commit statuses. A repo with no tracked PRs still resolves the
  repository, which doubles as an access check.
- **Job steps** ([spec 0007](specs/0007-check-tree.md)): `CheckRun.steps`
  nested under 100 check runs multiplies a query's rate-limit cost, so only
  the active tab's selected PR uses the `PrSteps` fragment
  (`Effect::FetchPrs { steps }`); the others use `Pr`. Selecting another PR
  forces a poll so its steps load right away.
- **Open PRs** (picker only, [spec 0008](specs/0008-manual-tracking.md)):
  one GraphQL request with the repo's 50 most recently updated open PRs
  and a `search` for the user's (`author:@me`), so the picker's mine/all
  switch needs no further request and no login lookup.
- **Logs**: `GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs`, which
  redirects to a short-lived download URL. A check run's GraphQL
  `databaseId` is the Actions job id. A 404 means "no log yet".
- **Errors**: GraphQL `NOT_FOUND` / `FORBIDDEN` / `RATE_LIMITED` and HTTP
  statuses map to `ApiError`. Repo-wide errors dim the tab; errors after a
  successful load go to the status bar and keep the last good data.

## Polling

See [spec 0008](specs/0008-manual-tracking.md). Every tab is fetched in
full once at start-up. After that a tab is polled every 30 s
(`CHECK_POLL`), active or not, only while some of its PRs have pending
checks, and only those PRs are fetched (`RepoState::pending`); the active
tab's selected PR gets its steps if it is among them. A tab with nothing
pending is not polled. A full fetch (`RepoState::full_fetch`) happens when
`force` is set — `r` on every tab, switching tabs, selecting or adding a
PR, after a rate limit — and on retries of the active tab when it failed
to load (every 60 s, `RETRY_POLL`).

**Manual refresh** ([spec 0006](specs/0006-refresh-all.md)): `r` sets
`force` on every repo. `App::refreshing` drives the tab bar's
`⟳ refreshing…` label until no repo is forced or in flight. An open log
window for a running Actions job re-fetches the log every 5 s and does one
final fetch after the job completes. A rate limit pauses all polling until
its reset time.

**Check notifications** ([spec 0004](specs/0004-check-notifications.md)):
`App::handle_prs` compares each PR's new detail with the previous one and
returns `Effect::Notify` when its checks went from pending to all complete.
Since pending PRs are exactly what is polled, this works in every tab.
`main.rs` sends it with `notify::send` (`osascript` on macOS, where
`notify-rust`'s notifications are dropped; `notify-rust` elsewhere) on a
short-lived thread, so a slow
notification service never blocks the loop; failures come back as
`Msg::NotifyFailed` and land in the status bar.

**Tracking is manual.** Nothing lists open PRs in the background and
merged or closed PRs are never dropped: the picker (`Picker::visible`
filters the listing by mine/all and number prefix) adds, `Ctrl+D` removes.

## Design rules

- **Rendering is read-only.** `ui` takes `&App` and never mutates it.
- **State transitions are pure.** `apply`, `handle` and `tick` return
  effects instead of doing I/O.
- **Input mapping is pure and context-free.** `event::map_key` doesn't know
  about modals; `App::apply` decides what a key means where.
- **Parsing is pure.** The octocrab layer only moves bytes; `github::parse`
  is tested with recorded fixtures.
- **UI is snapshot-testable** with `ratatui::backend::TestBackend`.
- **Auth and config errors happen before the TUI starts**, so they print
  normally. `ratatui::run` restores the terminal on exit and on panic.

## Testing

- `make test` runs everything offline.
- `cargo test -- --ignored live_` runs a smoke test against the real API
  (needs a token).

## Checks tree

`check_tree::rows(detail, folds)` turns a PR's flat, sorted checks into the
visible rows of the checks pane ([spec 0007](specs/0007-check-tree.md)):
Actions jobs grouped under their workflow run (`Check::run_id`), steps
under their job, other checks top-level. Runs and jobs that are running are
expanded unless the user's override in `RepoState::folds` (keyed by run or
job id) says otherwise. `RepoState::selected_check` is a row index into
this list; `App` and `ui` both call `RepoState::check_rows`, so key handling
and rendering always agree.

See [specs/](specs/) for feature-level behaviour.
