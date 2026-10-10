//! `werewolf-bot [--config <path>]`: reads `werewolf.toml` (writing a
//! commented one on first run), checks the token, opens the database
//! (migrating it if needed) and long-polls Telegram until Ctrl+C.
//!
//! The first Ctrl+C stops new games and waits for running ones to finish;
//! a second one ends them at once.

use bot::config::{self, Config};
use bot::core::{BotCore, CoreConfig, MemoryAdmission};
use bot::spawn::PoolSpawner;
use bot::telegram::{Retrying, Telegram};
use bot::teloxide_adapter::{poll, TeloxideTelegram};
use bot::text::Texts;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

fn config_path() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" | "-c" => path = Some(PathBuf::from(args.next().ok_or("--config needs a path")?)),
            "--help" | "-h" => {
                return Err(format!(
                    "usage: werewolf-bot [--config <path>]\n\n\
                     Without --config, reads {} next to the executable.\n\
                     The token can also come from the {} environment variable.",
                    config::FILE_NAME,
                    config::TOKEN_ENV
                ))
            }
            other => return Err(format!("unknown argument {other:?}; try --help")),
        }
    }
    Ok(path.unwrap_or_else(config::default_path))
}

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let config: Config = match config::load(&path, std::env::var(config::TOKEN_ENV).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let telegram = match TeloxideTelegram::connect(&config.token).await {
        Ok(t) => Arc::new(Retrying::new(t)),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    log::info!("logged in as @{}", telegram.bot_username());

    let store = match db::Store::connect(&config.database).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not open the database {}: {e}", config.database);
            return ExitCode::FAILURE;
        }
    };
    let (texts, failures) = Texts::load(&config.languages_dir);
    for f in &failures {
        log::warn!("language file skipped: {f}");
    }
    log::info!("{} languages loaded", texts.languages().len());

    let core = match BotCore::new(
        telegram.clone(),
        Arc::new(texts),
        store.clone(),
        Arc::new(PoolSpawner::new()),
        Box::new(MemoryAdmission::new(config.per_game_memory_mb * 1024 * 1024)),
        CoreConfig { default_language: config.default_language.clone(), global_admins: config.global_admins.clone(), seed: None },
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not load moderation state: {e}");
            return ExitCode::FAILURE;
        }
    };
    core.announce_interrupted_games().await;

    log::info!("running; press Ctrl+C to stop");
    // Polling keeps going while games wind down, so their players can
    // still press buttons; it stops once nothing is left running.
    let stop_polling = tokio_util::sync::CancellationToken::new();
    let poller = {
        let core = core.clone();
        let telegram = telegram.clone();
        let stop = stop_polling.clone();
        tokio::spawn(async move {
            poll(
                telegram.inner(),
                move |incoming| {
                    let core = core.clone();
                    tokio::spawn(async move { core.handle(incoming).await });
                },
                stop.cancelled_owned(),
            )
            .await
        })
    };

    let _ = tokio::signal::ctrl_c().await;
    core.stop_accepting();
    let running = core.active_groups();
    if running > 0 {
        log::info!("no new games; waiting for {running} to finish. Press Ctrl+C again to stop them now");
        tokio::select! {
            _ = core.wait_until_idle() => {}
            _ = tokio::signal::ctrl_c() => {
                core.kill_everything().await;
                // Let each game record its end before the database closes.
                let _ = tokio::time::timeout(std::time::Duration::from_secs(10), core.wait_until_idle()).await;
            }
        }
    }
    stop_polling.cancel();
    let _ = poller.await;
    store.close().await;
    log::info!("stopped");
    ExitCode::SUCCESS
}
