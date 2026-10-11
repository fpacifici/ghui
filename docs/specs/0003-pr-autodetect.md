# 0003 — PR autodetect

Status: Implemented

Extends [0002-pr-tracking.md](0002-pr-tracking.md). Adding your own PRs one
by one with `Ctrl+A` is tedious; ghui should notice them by itself.

## Goal

While a repo tab is open, PRs in that repo opened by the authenticated user
are tracked automatically, without touching the config by hand or opening
the picker.

## Behaviour

### Who "you" are

- On start-up ghui asks GitHub for the authenticated user's login
  (`GET /user`) once and keeps it for the session.
- If that fails it is retried every 60 s and the status bar shows the
  error. Autodetect is idle until the login is known.

### Detection

- Only the **active** tab is checked; inactive tabs are not.
- The active repo's open PRs are listed (the same listing as the picker:
  the 50 most recently updated) as soon as the login is known and the tab
  is shown, then every 60 s.
- Every open PR whose author is the authenticated user and that is neither
  tracked nor dismissed (below) is added to the end of the tab's PR list
  and saved to the config, exactly as if it had been picked with `Ctrl+A`.
  Its details are fetched right away. The selection does not move.
- A repo that failed to load (dimmed tab) is not checked.
- Rate limiting pauses detection together with all other polling.
- Opening the picker also counts as a detection pass: the open PRs it
  fetches are checked the same way.

### Dismissed PRs

- Untracking a PR (`d`) records it as **dismissed** for that repo, so
  autodetect never re-adds it.
- Adding a PR from the picker clears its dismissal.
- Dismissals are saved in the config next to the tracked PRs, so they
  survive restarts:

```yaml
repos:
  - name: owner/repo
    prs: [123]
    dismissed: [99]   # omitted when empty
```

## Key bindings

None. Autodetect has no key of its own; `d` and `Ctrl+A` behave as in
0002, plus the dismissal rules above.

## Acceptance criteria

- [x] The authenticated user's login is fetched once and cached; failures
      are retried every 60 s and reported in the status bar.
- [x] The active repo's open PRs are checked when the tab is shown and
      every 60 s after; inactive and failed repos are not.
- [x] Untracked open PRs by the authenticated user are added and saved.
- [x] Already tracked PRs and PRs by other authors are left alone.
- [x] Untracked PRs are dismissed and never re-added; picking one in the
      picker tracks it again and clears the dismissal.
- [x] Dismissals persist in the config.
- [x] Detection is unit tested without network access.

## Out of scope

- Detecting PRs beyond the 50 most recently updated open ones.
- Detecting PRs in repos that are not configured, or in inactive tabs.
- PRs where the user is a reviewer or assignee rather than the author.
- Notifying the user when a PR is added (it simply appears in the list).
- Un-dismissing a PR other than by picking it again.
