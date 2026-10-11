//! ghui — a terminal UI for GitHub.

mod app;
mod event;
mod ui;

use app::App;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::default().run(terminal))
}
