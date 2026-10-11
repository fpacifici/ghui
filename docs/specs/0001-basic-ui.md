# 0001 — Basic UI shell

Status: Implemented

## Goal

Provide the skeleton TUI that later GitHub features plug into: a tabbed
layout, a navigable list, and a key-binding footer.

## Behaviour

The screen has three regions, top to bottom:

1. **Tab bar** — bordered block titled `ghui` with tabs
   `Pull Requests`, `Issues`, `Notifications`. The active tab is highlighted.
2. **Body** — a bordered list titled with the active tab's name. Until the
   GitHub client exists, each tab shows five placeholder rows. The selected
   row is reversed and prefixed with `> `.
3. **Footer** — a single dim line listing the key bindings.

Switching tabs resets the selection to the first row. Moving the selection
past either end of the list clamps instead of wrapping.

## Key bindings

| Key                      | Action        |
|--------------------------|---------------|
| `q`, `Esc`, `Ctrl+C`     | Quit          |
| `Tab`, `→`, `l`          | Next tab      |
| `Shift+Tab`, `←`, `h`    | Previous tab  |
| `j`, `↓`                 | Move down     |
| `k`, `↑`                 | Move up       |

## Acceptance criteria

- [x] Tabs cycle in both directions and wrap around.
- [x] Selection is clamped to the list bounds and reset on tab change.
- [x] Quit restores the terminal to its previous state.
- [x] Rendering is covered by a `TestBackend` test.

## Out of scope

- Fetching real data from GitHub.
- Detail views, filtering, search, mouse support.
