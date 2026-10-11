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
   which may return `Effect`s too (e.g. saving autodetected PRs).
4. **Timers** — `App::tick(now)` returns the polls that are due.
5. **Effects** — network effects are spawned on the tokio runtime by
   `worker::Worker`; the rest (config save, clipboard, browser) run inline.

`App` never performs I/O. Everything it wants done is an `Effect`, and
everything that happened is an `Action` or a `Msg`, so all behaviour —
including the poll schedule, driven by an explicit `now` — is unit tested
without a terminal, network or real clock.

## Modules

| Module            | Responsibility                                                           | Side effects        |
|-------------------|--------------------------------------------------------------------------|---------------------|
| `main.rs`         | Auth + config before the TUI, tokio runtime, main loop, local effects.   | Terminal, files, stdout |
| `auth.rs`         | Token from `GH_TOKEN` → `GITHUB_TOKEN` → `gh auth token`.                | Runs `gh`           |
| `config.rs`       | `~/.ghui/config.yaml`: load, create if missing, atomic save, track/untrack/dismiss. | Files (load/save only) |
| `event.rs`        | `Action` enum, context-free `map_key`.                                    | Reads stdin events  |
| `app.rs`          | `App` state (repos, focus, modal, status), `apply` / `handle` / `tick`.  | None                |
| `log_view.rs`     | Log window state: content, scroll/follow, log poll bookkeeping.          | None                |
| `logs.rs`         | Cleans raw Actions logs (BOM, timestamps, ANSI, `\r`, endgroup).         | None                |
| `worker.rs`       | Turns network `Effect`s into tokio tasks that send back a `Msg`.         | Spawns tasks        |
| `github/mod.rs`   | `GitHub` trait and domain types (`PrDetail`, `Check`, `ApiError`, …).    | None                |
| `github/query.rs` | GraphQL documents.                                                       | None                |
| `github/parse.rs` | GraphQL JSON → domain types; tested against `tests/fixtures/`.           | None                |
| `github/octo.rs`  | `GitHub` implemented with octocrab; HTTP error mapping.                  | Network             |
| `ui/`             | Pure rendering: tabs, PR list, detail, checks, picker, log window.       | None                |

## GitHub access

- **PRs and checks**: one GraphQL request per repo, with one alias per
  tracked PR (`pr_123: pullRequest(number: 123)`). Each PR includes its
  head commit's `statusCheckRollup`, which carries both check runs and
  legacy commit statuses. A repo with no tracked PRs still resolves the
  repository, which doubles as an access check.
- **Open PRs** (picker and autodetect): GraphQL, 50 most recently updated.
- **Viewer login** (autodetect): `GET /user`, once per session.
- **Logs**: `GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs`, which
  redirects to a short-lived download URL. A check run's GraphQL
  `databaseId` is the Actions job id. A 404 means "no log yet".
- **Errors**: GraphQL `NOT_FOUND` / `FORBIDDEN` / `RATE_LIMITED` and HTTP
  statuses map to `ApiError`. Repo-wide errors dim the tab; errors after a
  successful load go to the status bar and keep the last good data.

## Polling

Only the active tab is polled. Its PRs are re-fetched every 5 s while the
selected PR has pending checks, every 60 s otherwise (and every 60 s for a
repo that failed to load). The interval is evaluated on each tick against
the current selection, so selecting a running PR speeds polling up
immediately. An open log window for a running Actions job re-fetches the log
every 5 s and does one final fetch after the job completes. A rate limit
pauses all polling until its reset time.

**Autodetect** ([spec 0003](specs/0003-pr-autodetect.md)) rides the same
`App::tick`: the first tick with a repo configured looks up the user's
login (`Effect::FetchViewer`, retried every 60 s on failure, then cached
in `App`). Once known, the active, non-failed repo's open PRs are listed
(`Effect::FetchOpenPrs`, the picker's effect) every 60 s. Every
`Msg::OpenPrs` — from autodetect or the picker — goes through
`App::autodetect`, which tracks the user's PRs that are neither tracked nor
dismissed and returns `Effect::SaveConfig`. `Config::untrack` records the
PR in the repo's `dismissed` list and `Config::track` clears it, so any
untrack path (UI key or otherwise) suppresses re-detection for free.

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

See [specs/](specs/) for feature-level behaviour.
