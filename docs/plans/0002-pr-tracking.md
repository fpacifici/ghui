# Plan — PR tracking (spec 0002)

Implementation plan for [specs/0002-pr-tracking.md](../specs/0002-pr-tracking.md).
Branch: `basic_interaction`. One commit per milestone; `make check` green at
each.

## Approach

- **Effects, not side effects.** `App::apply(Action)`, `App::handle(Msg)` and
  `App::tick(now)` stay pure and return `Vec<Effect>` (`FetchPr`,
  `FetchRepo`, `ListOpenPrs`, `FetchLog`, `SaveConfig`, `CopyUrl`,
  `OpenUrl`). The main loop executes them.
- **Async off the main thread.** A hand-built tokio runtime lives in `main`.
  `worker.rs` spawns a task per network effect, calls the `GitHub` trait and
  sends the result back as a `Msg` over an `mpsc` channel. The synchronous
  render → input → apply loop drains the channel every iteration (≤250 ms).
- **Pure scheduler.** `App::tick(now)` decides which polls are due, so polling
  is tested with a fake clock.

## Module layout

```
src/
├── main.rs        auth → load config → runtime → ratatui::run
├── auth.rs        GH_TOKEN → GITHUB_TOKEN → `gh auth token`
├── config.rs      ~/.ghui/config.yaml load/save/create-if-missing
├── github/
│   ├── mod.rs     GitHub trait + domain types
│   ├── query.rs   GraphQL query
│   ├── parse.rs   pure JSON → domain parsing (fixture tested)
│   └── octo.rs    octocrab implementation
├── worker.rs      Effect → spawned task → Msg channel
├── app.rs         state, apply / handle / tick → Vec<Effect>
├── event.rs       Action enum + context-free map_key
├── logs.rs        strip timestamps/ANSI, locate first ##[error]
└── ui/            mod, tabs, pr_list, detail, checks, picker, log_view
tests/fixtures/    GraphQL/REST JSON samples
```

New dependencies: `tokio`, `octocrab`, `serde`, `serde_json`,
`serde_yaml_ng` (maintained fork; `serde_yaml` is archived), `base64`
(OSC 52), `open`.

## Milestones

### M0 — Groundwork
- Worktree `.claude/worktrees/basic_interaction` on branch `basic_interaction`.
- Spec 0002 written; 0001 marked superseded; specs index updated.
- Log API probe against `fpacifici/assistant`. Findings:
  - A `CheckRun.databaseId` from GraphQL **is** the Actions job id
    (`GET /repos/{o}/{r}/actions/jobs/{id}` resolves it).
  - `GET .../actions/jobs/{id}/logs` returns the plain-text log (after a
    redirect) for completed jobs. Lines are prefixed with an ISO timestamp
    and the file starts with a UTF-8 BOM.
  - In-progress behaviour could not be observed (no running job). The client
    treats any content as a partial log and a 404 as "not available yet".

### M1 — Config and auth
- `Config { repos: Vec<RepoConfig { name, prs } > }`, load/save, create an
  empty file on first run.
- Token resolution with injectable env/command lookups.
- Tests: round-trip, missing file, malformed YAML, auth fallback order.

### M2 — GitHub layer
- Domain types: `PrDetail`, `PrSummary`, `Check { name, context, state,
  started_at, completed_at, kind }`, `CheckKind::{ActionsJob, CheckRun,
  Status}`, `LogState`.
- One GraphQL query per PR (title, body, author, branch, state, url,
  `statusCheckRollup` with check runs and status contexts).
- REST: 50 most recently updated open PRs; job log.
- `ApiError::{NotFound, Forbidden, RateLimited { reset }, Network, Other}`.
- Parsing tested against fixtures in `tests/fixtures/`.

### M3 — App state and scheduler
- `RepoState { name, status, prs }`, `active_repo`, `focus: List | Checks`,
  `selected_pr`, `selected_check`, `modal: Option<Picker | Log>`,
  `status_line`, `rate_limited_until`.
- Context-dependent key handling (Esc/q close modal; Enter per pane).
- Scheduler: load all PRs on first tab visit; selected PR every 5 s (60 s once
  all checks are done); log every 5 s while the job is running; `r` forces a
  refresh; nothing while rate limited.
- Check ordering: failing → pending → passing.

### M4 — Wiring
- `worker.rs` and `main.rs`: auth before the TUI (actionable error on
  failure), runtime, channel draining, `tick`.

### M5 — Main view UI
- Repo tabs (errored repos dimmed), PR list `● #123 Title [merged]`, detail
  header (author, branch, state, URL) + raw description (top third,
  truncated), checks table with summary line and elapsed time, status bar,
  footer, empty-config hint. `TestBackend` tests.

### M6 — Picker and untrack
- Ctrl+A modal, `✓` on tracked PRs, Enter tracks + saves + closes + selects;
  `d` untracks + saves. Save errors go to the status bar.

### M7 — Log view
- Full-screen overlay; cleaned lines; jump to first `##[error]` for failed
  jobs, else bottom; j/k, PgUp/PgDn, g/G; tail-follow while running unless
  scrolled up; non-Actions checks show summary + URL.

### M8 — URL actions, docs, verification
- `y` (OSC 52 copy), `o` (open PR or selected check).
- Update `docs/architecture.md` and the `AGENTS.md` tree; tick acceptance
  criteria.
- `make check`; manual run against `fpacifici/assistant`.

## Risks

- **Live logs.** The public API has no streaming endpoint; running jobs show
  whatever the log endpoint returns, or a placeholder.
- **GraphQL budget.** 5 s polling ≈ 720 requests/h for the selected PR only,
  well under 5,000 points/h.
- **Existing config.** Never overwritten; only created when missing.
