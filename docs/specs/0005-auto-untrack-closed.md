# 0005 — Untrack closed PRs automatically

Status: Implemented

Extends [0002-pr-tracking.md](0002-pr-tracking.md). Once a PR is merged or
closed there is nothing left to watch, and removing each one with `d` is
busywork.

## Goal

Merged and closed PRs disappear from the PR list and the config by
themselves.

## Behaviour

- When a poll (active or background, see
  [0004-check-notifications.md](0004-check-notifications.md)) returns a
  tracked PR whose state is **merged** or **closed**, the PR is removed from
  its tab's PR list and from the config, which is saved.
- This includes PRs already closed when ghui starts: they are removed on
  their tab's first load.
- If that same poll shows its checks finishing, the check notification is
  still sent.
- The PR is **not** dismissed (0003): autodetect only adds open PRs, so a
  PR that is later reopened by the user is tracked again.
- The selected PR stays selected. If the selected PR is the one removed,
  the selection stays at the same row, clamped to the shortened list, and
  moves to the first check, as with `d`.
- The status bar says which PRs were untracked, e.g.
  `o/r: untracked closed #12, #15`, until the next successful poll.
- A PR that fails to load is left alone.
- An open log window on a removed PR's check stays open.

## Key bindings

None.

## Acceptance criteria

- [x] Merged and closed PRs returned by a poll are removed from the list
      and the config, and the config is saved; open PRs are kept.
- [x] Removed PRs are not dismissed.
- [x] The selection follows the selected PR, or clamps if it was removed.
- [x] A finishing-checks notification for a PR closed in the same poll is
      still sent.
- [x] Unit tested without network access.

## Out of scope

- Keeping merged PRs around for a grace period, or a setting to turn this
  off.
- Removing repos whose PRs have all closed.
