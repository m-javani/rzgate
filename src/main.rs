// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

// main.rs
use clap::Parser;
use rzgate::{
    config::{Config, Mode},
    error::RZError,
    handler::handler::Handler,
    metrics::{Metrics, MetricsEvent},
    server,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::Level;
use tracing_subscriber::fmt::time::UtcTime;

/// RZGate - HTTP to Roomzin Binary Protocol Proxy
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to the YAML configuration file
    #[clap(short, long)]
    config: Option<String>,

    /// Listening address
    #[clap(short, long)]
    listening_addr: Option<String>,

    /// HTTP port
    #[clap(long)]
    http_port: Option<u16>,

    /// Server address (standalone host or router address)
    #[clap(long)]
    addr: Option<String>,

    /// TCP port
    #[clap(long)]
    port: Option<u16>,

    /// Mode: "standalone" or "router"
    #[clap(long)]
    mode: Option<String>,
}

fn find_config_path(cli_config: Option<&String>) -> Option<String> {
    if let Some(path) = cli_config {
        if Path::new(path).exists() {
            return Some(path.clone());
        }
    }
    if Path::new("rzgate.yml").exists() {
        return Some("rzgate.yml".to_string());
    }
    if Path::new("/etc/rzgate/rzgate.yml").exists() {
        return Some("/etc/rzgate/rzgate.yml".to_string());
    }
    None
}

fn main() -> Result<(), RZError> {
    // Setup tracing
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_timer(UtcTime::rfc_3339())
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_names(false)
        .with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");

    let args = Args::parse();

    // Find config path
    let config_path = find_config_path(args.config.as_ref())
        .ok_or_else(|| RZError::Config("No configuration file found".to_string()))?;

    // Load configuration
    let mut cfg = Config::load(&config_path)?;

    // Apply CLI overrides
    if let Some(la) = args.listening_addr {
        cfg.listening_addr = la;
    }
    if let Some(hp) = args.http_port {
        cfg.http_port = hp;
    }
    if let Some(addr) = args.addr {
        cfg.addr = addr;
    }
    if let Some(port) = args.port {
        cfg.port = port;
    }
    if let Some(mode_str) = args.mode {
        cfg.mode = match mode_str.as_str() {
            "standalone" => Mode::Standalone,
            "router" => Mode::Router,
            _ => return Err(RZError::Config(format!("Invalid mode: {}", mode_str))),
        };
    }

    // Calculate desired workers
    let desired_workers = if cfg.worker_threads == 0 {
        num_cpus::get_physical() * 3
    } else {
        cfg.worker_threads
    };

    tracing::debug!("Starting server with {desired_workers} Tokio worker threads");

    // Create main runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(desired_workers)
        .max_blocking_threads(512)
        .enable_all()
        .build()
        .map_err(|e| RZError::System(format!("Failed to build runtime: {e}")))?;

    // Run async main
    rt.block_on(async_main(cfg))
}

async fn async_main(cfg: Config) -> Result<(), RZError> {
    let shutdown = CancellationToken::new();

    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        tracing::info!("Ctrl+C received");
        shutdown_clone.cancel();
    });

    let (metrics_tx, metrics_rx) = tokio::sync::mpsc::channel::<MetricsEvent>(cfg.max_active_conns);

    let handler = Handler::new(cfg.clone(), metrics_tx.clone(), shutdown.clone());

    // Brief wait for handler to establish connections
    sleep(Duration::from_secs(1)).await;

    let metrics = Arc::new(Metrics::new());

    server::run(
        handler,
        cfg.listening_addr,
        cfg.http_port,
        shutdown,
        metrics_rx,
        metrics_tx,
        metrics,
    )
    .await
}
