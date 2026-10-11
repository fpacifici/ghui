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
│   ├── main.rs               # auth + config, tokio runtime, main loop, local effects
│   ├── auth.rs               # GH_TOKEN → GITHUB_TOKEN → `gh auth token`
│   ├── config.rs             # ~/.ghui/config.yaml load/save
│   ├── event.rs              # Action enum, terminal input → Action mapping
│   ├── app.rs                # App state; pure apply/handle/tick → Effects
│   ├── check_tree.rs         # checks pane rows: runs → jobs → steps
│   ├── log_view.rs           # log window state (scroll, follow, polling)
│   ├── scroll.rs             # scroll position that can follow the end
│   ├── api_log.rs            # log of the session's GitHub API calls
│   ├── logs.rs               # cleaning of raw Actions logs
│   ├── worker.rs             # network Effects → tokio tasks → Msg channel
│   ├── github/               # GitHub trait, types, GraphQL, parsing, octocrab
│   └── ui/                   # pure rendering of App into a Frame
├── tests/fixtures/           # recorded GitHub API responses for parser tests
├── docs/
│   ├── architecture.md       # modules, data flow, design rules
│   ├── plans/                # implementation plans for specs
│   └── specs/                # one spec per feature (NNNN-name.md)
└── .github/workflows/ci.yml  # CI: fmt, clippy, test, release build
```

## Conventions

- Keep the loop shape **render → input → apply → effects**. See
  [docs/architecture.md](docs/architecture.md).
- `ui` never mutates state. `App::apply`, `App::handle`, `App::tick` and
  `event::map_key` stay pure (no I/O; time is passed in) so they can be
  unit tested without a terminal, network or clock. Anything with a side
  effect is returned as an `Effect`.
- New key binding → add an `Action` variant, map it in `event::map_key`,
  handle it in `App::apply` (or `apply_modal`), update the footer and the
  relevant spec.
- New API call → add it to the `GitHub` trait, keep parsing in
  `github::parse` with a fixture under `tests/fixtures/`, and return it to
  the app as a `Msg` via `worker.rs`.
- Tests never hit the network. `cargo test -- --ignored live_` runs the
  live smoke test by hand.
- New feature → add or update a spec in `docs/specs/` first.
- Tests live next to the code in `#[cfg(test)] mod tests`. Use
  `ratatui::backend::TestBackend` for rendering tests.
- Use crossterm through `ratatui::crossterm` (no direct dependency) to
  avoid version mismatches.
- `make check` must pass before committing; CI runs the same steps on
  Linux and macOS.
