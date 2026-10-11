//! ghui — a terminal UI that tracks GitHub PRs and their checks.

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
use std::sync::mpsc::{self, Receiver, Sender};
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
    let worker = Worker::new(github, runtime.handle().clone(), tx.clone());
    let app = App::new(config, config_path.display().to_string(), Utc::now());
    let local = Local {
        config_path: &config_path,
        results: tx,
    };

    ratatui::run(|terminal| run(terminal, app, &worker, &rx, &local))
}

/// What local effects need from the outside world.
struct Local<'a> {
    config_path: &'a Path,
    /// Where local effects running in the background report failures.
    results: Sender<Msg>,
}

/// The render → input → apply loop, plus API results and timers.
fn run<G: GitHub>(
    terminal: &mut DefaultTerminal,
    mut app: App,
    worker: &Worker<G>,
    results: &Receiver<Msg>,
    local: &Local,
) -> color_eyre::Result<()> {
    while app.running {
        terminal.draw(|frame| ui::render(&app, frame))?;
        app.log_height = ui::log_view::height(terminal.size()?);
        if let Some(action) = event::next_action(INPUT_TIMEOUT)? {
            let effects = app.apply(action);
            execute(&mut app, worker, effects, local);
        }
        while let Ok(msg) = results.try_recv() {
            let effects = app.handle(msg, Utc::now());
            execute(&mut app, worker, effects, local);
        }
        let effects = app.tick(Utc::now());
        execute(&mut app, worker, effects, local);
    }
    Ok(())
}

fn execute<G: GitHub>(app: &mut App, worker: &Worker<G>, effects: Vec<Effect>, local: &Local) {
    for effect in effects {
        let Some(effect) = worker.spawn(effect) else {
            continue;
        };
        let result = match effect {
            Effect::SaveConfig(config) => config
                .save(local.config_path)
                .map_err(|e| format!("cannot save config: {e}")),
            Effect::CopyToClipboard(text) => {
                copy_to_clipboard(&text).map_err(|e| format!("cannot copy: {e}"))
            }
            Effect::OpenUrl(url) => {
                open::that_detached(&url).map_err(|e| format!("cannot open {url}: {e}"))
            }
            Effect::Notify { title, body } => {
                // Sending may wait on D-Bus or Notification Center; never
                // block the UI on it.
                let results = local.results.clone();
                std::thread::spawn(move || {
                    if let Err(e) = notify(&title, &body) {
                        let _ = results.send(Msg::NotifyFailed(format!("cannot notify: {e}")));
                    }
                });
                Ok(())
            }
            Effect::FetchPrs { .. }
            | Effect::FetchOpenPrs { .. }
            | Effect::FetchViewer
            | Effect::FetchLog { .. } => unreachable!("network effects are spawned"),
        };
        if let Err(message) = result {
            app.report_error(message);
        }
    }
}

/// Shows a native desktop notification.
fn notify(title: &str, body: &str) -> Result<(), notify_rust::error::Error> {
    // On macOS the notification is sent when the returned handle drops.
    notify_rust::Notification::new()
        .appname("ghui")
        .summary(title)
        .body(body)
        .show()
        .map(drop)
}

/// Copies through the terminal with an OSC 52 escape sequence, which also
/// works over SSH in most terminals.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = std::io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}
