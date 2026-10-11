# 0008 — Manual tracking and pending-only polling

Status: Implemented

Supersedes [0003-pr-autodetect.md](0003-pr-autodetect.md) and
[0005-auto-untrack-closed.md](0005-auto-untrack-closed.md); replaces the
polling rules of [0002-pr-tracking.md](0002-pr-tracking.md),
[0004-check-notifications.md](0004-check-notifications.md) and
[0006-refresh-all.md](0006-refresh-all.md). Background polling for new and
closed PRs spent API quota on things the user can do in two key presses,
and polling PRs whose checks had finished spent it on data that does not
change.

## Goal

The user decides which PRs are tracked; ghui only polls what can still
change on its own: checks that are pending.

## Behaviour

### Adding PRs (`Ctrl+A`)

- The picker is the only way to add PRs besides editing the config. There
  is no autodetect.
- Opening it makes one request listing both the 50 most recently updated
  open PRs of the repo (**all**) and up to 50 open PRs authored by the
  user (**mine**, a GitHub search for `author:@me`, most recently updated
  first).
- It opens on **mine**. `m` switches between mine and all; the title says
  which is shown, e.g. `Add a PR to o/r — mine (m: all)`.
- Typing digits fills a filter bar at the bottom of the picker:
  `#12▏`. Only PRs whose number starts with the typed digits are shown.
  `Backspace` deletes the last digit. Switching mine/all keeps the filter.
- Already tracked PRs are marked `✓`; `Enter` on an untracked PR tracks
  it as before.
- When the filter matches nothing, the list says so and `Enter` tracks
  PR `#<filter>` directly, so PRs outside the listing (or while it is
  loading or failed) can still be added. A number that does not exist
  shows `not found` in the PR list, like any other.
- `q` / `Esc` close the picker; the filter is not kept.

### Removing PRs (`Ctrl+D` / `d`)

- Unchanged from 0002: untracks the selected PR.
- Merged and closed PRs are **not** removed automatically. They stay in
  the list with a dimmed `merged` / `closed` tag until untracked.
- There are no dismissals any more: an old config's `dismissed` lists are
  ignored and dropped on the next save.

### Polling

- On start-up every tab is fetched once.
- Afterwards a tab is polled every **30 s**, active or not, while any of
  its PRs has pending checks. Only those PRs are fetched. Tabs with no
  pending checks are not polled at all.
- The active tab's selected PR is fetched with its job steps (0007) when
  it is part of the fetch.
- A full fetch of a tab (all its PRs) happens on start-up, on `r` (every
  tab), when switching to a tab, when selecting another PR and after
  adding one. This is how a new push to a PR whose checks had completed
  is noticed.
- A tab that failed to load is retried every 60 s while it is active.
- `r` no longer lists open PRs or removes closed ones; it only re-fetches
  every tab.
- Notifications (0004) are unchanged: a PR whose checks go from pending
  to complete notifies, in any tab.
- An open log window and rate limiting behave as before.

## Key bindings

| Key            | Where  | Action                         |
|----------------|--------|--------------------------------|
| `m`            | picker | Switch between mine and all    |
| `0`–`9`        | picker | Type into the number filter    |
| `Backspace`    | picker | Delete the last filter digit   |
| `Ctrl+D`, `d`  | list   | Untrack the selected PR        |

## Acceptance criteria

- [x] No request is made in the background to list open PRs or to look up
      the user; the picker's listing is the only one.
- [x] Merged and closed PRs stay tracked and are tagged in the PR list.
- [x] The picker opens on the user's PRs; `m` toggles all; digits filter
      by number prefix; `Backspace` edits the filter.
- [x] `Enter` with a filter matching nothing tracks that number.
- [x] Tabs are polled every 30 s only while they have pending checks, and
      only their pending PRs are fetched.
- [x] Start-up, `r`, switching tabs and selecting a PR fetch the whole
      tab.
- [x] Pending → complete still notifies, in background tabs too.
- [x] Unit tested without network access.

## Out of scope

- Noticing a new push on a PR with completed checks without a full fetch
  (`r`, switching tab, selecting it).
- Filtering the picker by title or author.
- Paging beyond 50 PRs in either listing.
