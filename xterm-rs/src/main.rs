// run  := cargo run -- --resource /home/jyh/project/xterm-rs/static
// dir  := .
// kid  :=
use anyhow::Context;
use axum::{Extension, Router, routing::get};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::services::ServeDir;

mod caster;
mod config;
mod index;
mod models;
mod pty;
mod sockets;

use index::index;

use caster::Caster;
use config::spawn_cfg_watcher;
use models::{AppState, logger};
use pty::PtyManager;
use sockets::{ws_handler, ws_handler_debug};

use clap::{Parser, ValueHint};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = "TODO")]
struct Args {
    #[arg(
        short,
        long,
        default_value = "/bin/bash",
        long_help = "Command to run in the terminal"
    )]
    command: String,

    #[arg(long, default_value_t = 24u16, long_help = "Terminal initial rows")]
    rows: u16,

    #[arg(long, default_value_t = 80u16, long_help = "Terminal initial columns")]
    cols: u16,

    #[arg(long, value_hint=ValueHint::DirPath, long_help = "Path to static files")]
    resource: std::path::PathBuf,

    #[arg(
        long,
        value_hint = ValueHint::DirPath,
        default_value = "/home/student/.local/state/workspace-logs/",
    )]
    log_dir: std::path::PathBuf,

    #[arg(
        long,
        value_hint = ValueHint::DirPath,
        default_value = "/home/student/.config/config.toml"
    )]
    config_path: std::path::PathBuf,

    #[arg(short, long, default_value_t = 8080usize, long_help = "Port to listen on")]
    port: usize,

    #[arg(
        long,
        default_value_t = 4194304usize, // 4MB
        long_help = "Terminal history buffer limit (bytes)"
    )]
    history_limit: usize,

    #[arg(
        long,
        default_value_t = 0u8,
        value_parser = clap::value_parser!(u8).range(0..=2),
        long_help = "Log verbosity level:\n  0 = none\n  1 = cast files\n  2 = cast files & stdout"
    )]
    log_level: u8,

    #[arg(
        long,
        default_value_t = 120u32,
        value_parser = clap::value_parser!(u32).range(10..=3600),
        long_help = "Verbose log interval (s)\nOnly used when log_level is 2"
    )]
    verbose_interval: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let pty = Arc::new(PtyManager::new(args.rows, args.cols, args.history_limit).await?);

    let ts_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_millis();
    let start = std::time::Instant::now();

    let caster = match args.log_level {
        0 => None,
        x => Some(Caster::new(
            args.log_dir,
            start,
            ts_millis,
            x == 2,
            args.verbose_interval,
            (args.rows, args.cols),
        )?),
    };

    let (cfg_watcher, _join) = spawn_cfg_watcher(args.config_path).await?;

    let state = Arc::new(AppState {
        start,
        pty: Arc::clone(&pty),
        caster,
        watcher: cfg_watcher,
        stty_size: Arc::new(tokio::sync::RwLock::new((args.rows, args.cols))),
    });

    let app = Router::new()
        .nest_service("/static", ServeDir::new(args.resource))
        .route("/ws", get(ws_handler))
        .route("/", get(index))
        .route("/debug", get(index))
        .route("/debug/ws", get(ws_handler_debug))
        .layer(Extension(state));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.port)).await?;

    logger("info", format!("Listening on http://{}", listener.local_addr()?));

    axum::serve(listener, app).await.context("server error")
}
