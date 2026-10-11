# 0005 — Enter opens the browser

Status: Implemented

Extends [0002-pr-tracking.md](0002-pr-tracking.md). The public API exposes no
live log stream, so the log view of a running job only says "Job in progress".
The live log is on github.com; getting there should take one key.

## Goal

`Enter` takes the user to the most useful place for what is selected: the
PR page for a PR, the live check page for a running check, and the cleaned
log view for a finished check.

## Behaviour

- **PR list focused**: `Enter` opens the selected PR's URL in the default
  browser. Moving focus to the checks pane stays on `→` / `l`.
- **Checks pane focused, pending check**: `Enter` opens the check's URL in
  the default browser (for Actions jobs, the live job page).
- **Checks pane focused, completed check** (passed, failed, skipped):
  `Enter` opens the log view, as before.
- A pending check without a URL falls back to the log view.
- Nothing selected (no PR loaded, no checks): `Enter` does nothing.
- `o` is unchanged: it always opens the PR, or the focused check, in the
  browser.

## Key bindings

| Key     | Context            | Action                                         |
|---------|--------------------|------------------------------------------------|
| `Enter` | PR list            | Open the PR in the browser                     |
| `Enter` | Checks             | Running check: open in browser; else log view  |

## Acceptance criteria

- [x] `Enter` on a PR returns an open-URL effect with the PR URL and leaves
      focus on the list.
- [x] `Enter` on a pending check with a URL returns an open-URL effect with
      the check URL and opens no modal.
- [x] `Enter` on a completed check (or a pending one without a URL) opens
      the log view.
- [x] Footers describe the new `Enter` behaviour.

## Out of scope

- Streaming live logs inside the TUI.
- Configuring which browser is used (the OS default is used, as for `o`).
