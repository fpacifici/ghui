# 0009 — API call log

Status: Implemented

## Goal

See every request ghui makes to GitHub, to understand and tune its API
usage (see [0008-manual-tracking.md](0008-manual-tracking.md)).

## Behaviour

- Every GitHub API request is recorded once it completes, successful or
  not: the time it was sent, the HTTP method and path, what it asked for,
  and its outcome. Response bodies are not recorded.
- One line per call, oldest first:

  ```
  12:03:04  POST /graphql                         PRs o/a #1 #2 (steps #1)  ok
  12:03:09  POST /graphql                         open PRs o/a              rate limited
  12:03:11  GET  /repos/o/a/actions/jobs/7/logs   job log                   ok
  ```

  - GraphQL requests say what they fetch: `PRs <repo> #n …` (with
    `(steps #n)` for the PR fetched with job steps), `repo <repo>` when no
    PR is tracked, or `open PRs <repo>` for the picker.
  - The outcome is `ok`, or the error code: `not found`, `forbidden`,
    `rate limited`, `network error`, the HTTP status (e.g. `502`), or
    `error`. Errors are red.
  - Times are UTC, as in the rate-limit message.
- `L` opens the log as a full-screen window over the main view. It
  follows new calls at the bottom, like `tail -f`, until scrolled up;
  scrolling back to the end follows again. The title shows the number of
  calls, the bottom border the visible range.
- The last 1000 calls of the session are kept; nothing is written to disk.

## Key bindings

| Key                 | Where       | Action                         |
|---------------------|-------------|--------------------------------|
| `L`                 | main view   | Open the API log               |
| `j`/`k`, `↑`/`↓`    | API log     | Scroll one line                |
| `PgUp`/`PgDn`       | API log     | Scroll one page                |
| `g`/`G`             | API log     | Top / bottom (follow)          |
| `Esc`, `q`          | API log     | Close                          |

## Acceptance criteria

- [x] Every PR fetch, open-PR listing and job-log request is logged with
      time, method, path, description and outcome code.
- [x] `L` opens a scrollable window that follows new calls at the end.
- [x] At most 1000 calls are kept.
- [x] Unit tested without network access.

## Out of scope

- Request durations, rate-limit headers, response bodies.
- Persisting the log or exporting it.
- Recording the download that follows a job log's redirect (not a GitHub
  API call).
