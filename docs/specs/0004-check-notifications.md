# 0004 — Check notifications

Status: Implemented

> The polling rules below are replaced by
> [0008-manual-tracking.md](0008-manual-tracking.md).

Extends [0002-pr-tracking.md](0002-pr-tracking.md). Waiting for CI means
either staring at the TUI or forgetting to come back to it; ghui should tell
the user when a watched PR's checks are done.

## Goal

When all checks on a tracked PR finish, a native desktop notification says
whether they passed or failed, even when that PR is not selected or its
repo tab is not active.

## Behaviour

### When a notification is sent

- A PR notifies when a poll shows **no pending checks** and the previous
  poll of the same PR showed **at least one** pending check. In other words,
  ghui notifies only on a transition it observed: PRs whose checks were
  already complete when first loaded (e.g. at start-up) stay silent.
- One notification per transition. A new push restarts the checks; when
  they finish again, the PR notifies again.
- PRs with no checks at all never notify.
- A PR that fails to load keeps its previous state; it notifies once a later
  poll succeeds and shows the checks complete.

### Content

- Title: `Checks passed` when no check failed, otherwise `Checks failed`.
- Body: `owner/repo #123 PR title`, followed on failure by the number of
  failing checks, e.g. `2 of 10 checks failed`.

### Delivery

- Native OS notification: Notification Center on macOS, the D-Bus
  notification service on Linux.
- Sending never blocks the UI. If the system refuses the notification (e.g.
  no notification service on Linux) the error is shown in the status bar.
  macOS does not report delivery failures.

### Polling every tracked repo

Before this spec only the active tab was polled, which would hide check
completions in other tabs. Now:

- The **active** tab is polled exactly as in 0002 (5 s while the selected PR
  has pending checks, 60 s otherwise).
- An **inactive** tab is fetched once at start-up, then every 60 s while any
  of its PRs has pending checks. Once all its checks are complete it is no
  longer polled in the background.
- Switching to a tab refreshes it immediately, since background polls may
  have left it up to 60 s stale.
- Inactive tabs that failed to load (dimmed) are not polled in the
  background.
- Rate limiting pauses background polling together with all other polling.

## Key bindings

None.

## Acceptance criteria

- [x] A PR whose checks go from pending to complete sends exactly one
      notification, saying passed or failed.
- [x] PRs loaded with checks already complete, and PRs without checks, do
      not notify.
- [x] A rerun (complete → pending → complete) notifies again.
- [x] Inactive tabs are fetched at start-up, then every 60 s only while
      they have pending checks; failed inactive tabs are not polled.
- [x] Switching to a tab refreshes it immediately.
- [x] Notification failures are reported in the status bar.
- [x] Detection and the poll schedule are unit tested without network
      access or a notification service.

## Out of scope

- Turning notifications off, or per-PR / per-repo settings.
- Notifying on the first failing check before the rest finish.
- Notifications over SSH or via terminal escape sequences (OSC 9/777).
- Clicking the notification to focus ghui or open the PR.
- Notifying about other events (reviews, merges, new comments).
