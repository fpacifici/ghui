//! ghui — a terminal UI for GitHub.

mod app;
#[allow(dead_code)] // wired up in M4
mod auth;
#[allow(dead_code)] // wired up in M4
mod config;
mod event;
#[allow(dead_code)]
mod github;
mod ui;

use app::App;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))
}
