//! ghui — a terminal UI that tracks GitHub PRs and their checks.

// Parts of the GitHub layer are used by the picker and log view (M6-M7).
#![allow(dead_code)]

mod app;
mod auth;
mod config;
mod event;
mod github;
mod log_view;
mod logs;
mod ui;
mod worker;

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use base64::Engine;
use chrono::Utc;
use color_eyre::eyre::eyre;
use ratatui::DefaultTerminal;

use app::{App, Effect, Msg};
use config::Config;
use github::GitHub;
use github::octo::OctoGitHub;
use worker::Worker;

/// How long to wait for input before handling API results and timers.
const INPUT_TIMEOUT: Duration = Duration::from_millis(100);

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    // Fail before touching the terminal, so the message stays readable.
    let token = auth::token().ok_or_else(|| eyre!(auth::MISSING_TOKEN))?;
    let config_path = config::default_path()?;
    let config = Config::load_or_create(&config_path)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let github = {
        let _guard = runtime.enter();
        OctoGitHub::new(token).map_err(|e| eyre!("cannot create the GitHub client: {e}"))?
    };
    let (tx, rx) = mpsc::channel();
    let worker = Worker::new(github, runtime.handle().clone(), tx);
    let app = App::new(config, config_path.display().to_string(), Utc::now());

    ratatui::run(|terminal| run(terminal, app, &worker, &rx, &config_path))
}

/// The render → input → apply loop, plus API results and timers.
fn run<G: GitHub>(
    terminal: &mut DefaultTerminal,
    mut app: App,
    worker: &Worker<G>,
    results: &Receiver<Msg>,
    config_path: &Path,
) -> color_eyre::Result<()> {
    while app.running {
        terminal.draw(|frame| ui::render(&app, frame))?;
        app.log_height = ui::log_view::height(terminal.size()?);
        if let Some(action) = event::next_action(INPUT_TIMEOUT)? {
            let effects = app.apply(action);
            execute(&mut app, worker, effects, config_path);
        }
        while let Ok(msg) = results.try_recv() {
            app.handle(msg, Utc::now());
        }
        let effects = app.tick(Utc::now());
        execute(&mut app, worker, effects, config_path);
    }
    Ok(())
}

fn execute<G: GitHub>(app: &mut App, worker: &Worker<G>, effects: Vec<Effect>, config_path: &Path) {
    for effect in effects {
        let Some(effect) = worker.spawn(effect) else {
            continue;
        };
        let result = match effect {
            Effect::SaveConfig(config) => config
                .save(config_path)
                .map_err(|e| format!("cannot save config: {e}")),
            Effect::CopyToClipboard(text) => {
                copy_to_clipboard(&text).map_err(|e| format!("cannot copy: {e}"))
            }
            Effect::OpenUrl(url) => {
                open::that_detached(&url).map_err(|e| format!("cannot open {url}: {e}"))
            }
            Effect::FetchPrs { .. } | Effect::FetchOpenPrs { .. } | Effect::FetchLog { .. } => {
                unreachable!("network effects are spawned")
            }
        };
        if let Err(message) = result {
            app.report_error(message);
        }
    }
}

/// Copies through the terminal with an OSC 52 escape sequence, which also
/// works over SSH in most terminals.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = std::io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}
