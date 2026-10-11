# 0006 — Refresh every tab on demand

Status: Implemented

> The new-PR detection and closed-PR removal below are replaced by
> [0008-manual-tracking.md](0008-manual-tracking.md).

Extends [0002-pr-tracking.md](0002-pr-tracking.md),
[0003-pr-autodetect.md](0003-pr-autodetect.md) and
[0005-auto-untrack-closed.md](0005-auto-untrack-closed.md). New PRs are only
detected in the active tab, and merged or closed PRs are only dropped when
their tab is polled, which for a quiet inactive tab is never. Rather than
polling everything in the background, the user asks for it.

## Goal

One key brings every tab up to date: new PRs by the user appear, and merged
or closed PRs disappear, in all repos.

## Behaviour

- `r` in the main view refreshes **every** repo tab, active or not:
  - its tracked PRs are fetched right away, so merged and closed PRs are
    removed (0005) and finished checks notify (0004);
  - its open PRs are listed right away, so the user's new PRs are added
    (0003), once the user's login is known.
- Repos that failed to load (dimmed tabs) are retried too; their open PRs
  are listed once they load.
- Each refresh lists each repo once; the regular schedule (0002, 0003,
  0004) is otherwise unchanged.
- The tab bar shows the refresh "button" at its top right: `⟳ r refresh`,
  which reads `⟳ refreshing…` until every tab's PR fetch has come back.
- In the log window `r` still refreshes only the log (0002).

## Key bindings

| Key | Action                                           |
|-----|--------------------------------------------------|
| `r` | Refresh every tab: fetch PRs and look for new ones |

## Acceptance criteria

- [x] `r` fetches every tab's PRs, including inactive and failed ones, and
      removes merged and closed PRs.
- [x] `r` lists every loaded tab's open PRs once and tracks the user's new
      ones.
- [x] The tab bar shows the refresh button and a refreshing state.
- [x] Unit tested without network access.

## Out of scope

- Clicking the button with the mouse (no mouse support, 0002).
- Background polling of every tab.
