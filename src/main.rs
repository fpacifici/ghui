//! ghui — a terminal UI for GitHub.

#[allow(dead_code)] // wired up in M4
mod app;
#[allow(dead_code)] // wired up in M4
mod auth;
#[allow(dead_code)] // wired up in M4
mod config;
mod event;
#[allow(dead_code)] // wired up in M4
mod github;
mod ui;

use std::time::Duration;

use app::App;
use config::Config;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let config = Config::load_or_create(&config::default_path()?)?;
    let mut app = App::new(config, chrono::Utc::now());
    ratatui::run(|terminal| {
        while app.running {
            terminal.draw(|frame| ui::render(&app, frame))?;
            if let Some(action) = event::next_action(Duration::from_millis(250))? {
                app.apply(action);
            }
            app.tick(chrono::Utc::now());
        }
        Ok(())
    })
}
