# AGENTS.md

Guidance for AI coding agents (and humans) working on **ghui**, a GitHub TUI
written in Rust with [ratatui](https://ratatui.rs).

## Quick start

```sh
make setup     # install toolchain components via rustup, fetch deps
make run       # launch the TUI (q to quit)
make check     # fmt-check + clippy + tests — run before every commit
```

## Make targets

| Target           | What it does                                              |
|------------------|-----------------------------------------------------------|
| `make help`      | List all targets                                          |
| `make setup`     | Ensure the toolchain from `rust-toolchain.toml` (+ rustfmt, clippy) is installed; `cargo fetch` |
| `make build`     | Debug build                                               |
| `make release`   | Optimized build → `target/release/ghui`                   |
| `make run`       | Run the app in debug mode                                 |
| `make test`      | `cargo test --all-targets`                                |
| `make fmt`       | Format code                                               |
| `make fmt-check` | Verify formatting (no changes)                            |
| `make lint`      | `cargo clippy --all-targets -- -D warnings`               |
| `make check`     | `fmt-check` + `lint` + `test` — same as CI                |
| `make clean`     | `cargo clean`                                             |

## Repository structure

```
.
├── AGENTS.md                 # this file
├── Cargo.toml / Cargo.lock   # crate manifest and locked deps
├── Makefile                  # dev workflow entry points
├── rust-toolchain.toml       # pinned toolchain (stable + rustfmt, clippy)
├── src/
│   ├── main.rs               # entry point: color_eyre + ratatui::run
│   ├── app.rs                # App state, Tab enum, main loop, apply(Action)
│   ├── event.rs              # Action enum, terminal input → Action mapping
│   └── ui.rs                 # pure rendering of App into a Frame
├── docs/
│   ├── architecture.md       # modules, data flow, design rules
│   └── specs/                # one spec per feature (NNNN-name.md)
└── .github/workflows/ci.yml  # CI: fmt, clippy, test, release build
```

## Conventions

- Keep the loop shape **render → input → apply**. See
  [docs/architecture.md](docs/architecture.md).
- `ui` never mutates state; `App::apply` and `event::map_key` stay pure so
  they can be unit tested without a terminal.
- New key binding → add an `Action` variant, map it in `event::map_key`,
  handle it in `App::apply`, update the footer and the relevant spec.
- New feature → add or update a spec in `docs/specs/` first.
- Tests live next to the code in `#[cfg(test)] mod tests`. Use
  `ratatui::backend::TestBackend` for rendering tests.
- Use crossterm through `ratatui::crossterm` (no direct dependency) to
  avoid version mismatches.
- `make check` must pass before committing; CI runs the same steps on
  Linux and macOS.
