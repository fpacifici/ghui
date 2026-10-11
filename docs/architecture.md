# Architecture

ghui is a single-binary Rust application that renders a terminal UI with
[ratatui](https://ratatui.rs) on top of the crossterm backend.

## Overview

```
          ┌──────────────┐   KeyEvent    ┌──────────────┐
terminal ─▶  event.rs    ├──────────────▶│  Action      │
 input    │ (poll/read,  │   map_key()   │ (enum)       │
          │  key mapping)│               └──────┬───────┘
          └──────────────┘                      │ App::apply()
                                                ▼
          ┌──────────────┐   &App        ┌──────────────┐
terminal ◀┤   ui.rs      │◀──────────────┤   app.rs     │
 output   │ (render only)│   render()    │ (state +     │
          └──────────────┘               │  main loop)  │
                                         └──────────────┘
```

The app follows a simple **Elm-style loop**:

1. **Draw** — `ui::render(&app, frame)` turns the current state into widgets.
2. **Wait for input** — `event::next_action()` polls crossterm for up to
   250 ms and maps a key press to an `Action` (or `None`).
3. **Update** — `App::apply(action)` mutates state.
4. Repeat until `app.running` is `false`.

## Modules

| Module          | Responsibility                                                     | Side effects          |
|-----------------|--------------------------------------------------------------------|-----------------------|
| `src/main.rs`   | Installs `color_eyre`, sets up/restores the terminal via `ratatui::run`. | Terminal setup        |
| `src/app.rs`    | `App` state, `Tab` enum, the main loop, `apply()` state transitions. | None in `apply()`     |
| `src/event.rs`  | `Action` enum, reading terminal events, `map_key()` key bindings. | Reads stdin events    |
| `src/ui.rs`     | Pure rendering of `App` into a `Frame`.                            | None                  |

## Design rules

- **Rendering is read-only.** `ui` takes `&App` and never mutates it.
- **State transitions are pure.** `App::apply` takes an `Action`, so behaviour
  can be unit tested without a terminal.
- **Input mapping is pure.** `event::map_key` is separate from the I/O in
  `next_action` so key bindings are testable.
- **UI is snapshot-testable** with `ratatui::backend::TestBackend`
  (see `ui::tests`).
- **Terminal restore is automatic.** `ratatui::run` restores the terminal on
  exit and on panic; `color_eyre` provides readable error reports.

## Planned evolution

- A `github` module wrapping the GitHub REST/GraphQL API (likely `octocrab`),
  authenticated via `GH_TOKEN` / `GITHUB_TOKEN` or the `gh` CLI's stored token.
- Async data loading (tokio) feeding results back to the loop as
  `Action`s/messages, so the UI never blocks on the network.
- Per-tab state structs once tabs diverge (filters, detail views, pagination).

See [specs/](specs/) for feature-level behaviour.
