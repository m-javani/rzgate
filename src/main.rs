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
    auth,
    bitmask::set_codecs,
    config::Config,
    error::RZError,
    handler::{self, handler::Handler},
    metrics::{Metrics, MetricsEvent},
    processor::{base::Codecs, get_codecs::process_get_codecs},
    server,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::Level;
use tracing_subscriber::fmt::time::UtcTime;

/// RZGate - API Gateway Server
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to the YAML configuration file.
    #[clap(short, long)]
    config: Option<String>,

    #[clap(short = 'a', long)]
    auth_enabled: Option<bool>,

    #[clap(long)]
    http: Option<bool>,

    #[clap(long)]
    https: Option<bool>,

    /// Path to the auth tokens file
    #[clap(short = 't', long)]
    tokens_path: Option<String>,

    #[clap(long)]
    tls_cert: Option<String>,

    #[clap(long)]
    tls_key: Option<String>,

    /// Discovery kind: "static" or "http"
    #[clap(long)]
    discovery_kind: Option<String>,

    /// Path to discovery YAML file (for static mode)
    #[clap(long)]
    discovery_yml_path: Option<String>,

    /// Discovery service address (for http mode)
    #[clap(long)]
    discovery_addr: Option<String>,

    // === New arguments for standalone / no_cluster mode ===
    /// Run in standalone (no cluster) mode
    #[clap(long)]
    no_cluster: Option<bool>,

    /// Address of the Roomzin server when running in standalone mode
    #[clap(long)]
    roomzin_standalone_host: Option<String>,
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

    // === CLI Argument Validation (Conflict Check) ===
    let using_no_cluster_cli = args.no_cluster == Some(true);
    let has_cluster_args = args.discovery_kind.is_some()
        || args.discovery_yml_path.is_some()
        || args.discovery_addr.is_some();

    if using_no_cluster_cli && has_cluster_args {
        return Err(RZError::Config(
            "Cannot combine --no-cluster with discovery arguments (--discovery-kind, --discovery-yml-path, --discovery-addr)".into()
        ));
    }

    if !using_no_cluster_cli && args.roomzin_standalone_host.is_some() {
        return Err(RZError::Config(
            "--roomzin-standalone-addr can only be used together with --no-cluster".into(),
        ));
    }

    // === Apply CLI Overrides ===
    if let Some(kind) = args.discovery_kind {
        cfg.discovery_kind = kind
            .parse()
            .map_err(|e| RZError::Config(format!("Invalid discovery_kind: {}", e)))?;
    }
    if let Some(path) = args.discovery_yml_path {
        cfg.discovery_yml_path = Some(path);
    }
    if let Some(addr) = args.discovery_addr {
        cfg.discovery_addr = Some(addr);
    }
    if let Some(v) = args.auth_enabled {
        cfg.auth_enabled = v;
    }
    if let Some(v) = args.http {
        cfg.http_enabled = v;
    }
    if let Some(v) = args.https {
        cfg.https_enabled = v;
    }

    // No-cluster mode overrides
    if let Some(v) = args.no_cluster {
        cfg.no_cluster = v;
    }
    if let Some(addr) = args.roomzin_standalone_host {
        cfg.roomzin_standalone_host = Some(addr);
    }

    // Final validation after overrides
    if cfg.no_cluster && cfg.roomzin_standalone_host.is_none() {
        return Err(RZError::Config(
            "roomzin_standalone_host is required when no_cluster = true".into(),
        ));
    }

    // Find auth path (CLI > Config > Defaults)
    let tokens_path = if let Some(path) = args.tokens_path {
        path
    } else if !cfg.tokens_path.is_empty() {
        cfg.tokens_path.clone()
    } else if Path::new("auth.yml").exists() {
        "auth.yml".to_string()
    } else if Path::new("/etc/rzgate/auth.yml").exists() {
        "/etc/rzgate/auth.yml".to_string()
    } else {
        return Err(RZError::Config("No auth file found".to_string()));
    };

    // Initialize tokens synchronously
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| RZError::System(format!("Failed to build runtime: {e}")))?;

    rt.block_on(async {
        auth::init_tokens(&tokens_path)
            .await
            .map_err(|e| RZError::Config(format!("Auth error: {}", e)))
    })?;

    let (mut tls_cert_path, mut tls_key_path) = (None, None);
    if cfg.https_enabled {
        tls_cert_path = Some(args.tls_cert.unwrap_or_else(|| cfg.tls_cert_path.clone()));
        tls_key_path = Some(args.tls_key.unwrap_or_else(|| cfg.tls_key_path.clone()));
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
    rt.block_on(async_main(cfg, tokens_path, tls_cert_path, tls_key_path))
}

async fn async_main(
    cfg: Config,
    tokens_path: String,
    tls_cert_path: Option<String>,
    tls_key_path: Option<String>,
) -> Result<(), RZError> {
    let shutdown = CancellationToken::new();

    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        tracing::info!("Ctrl+C received");
        shutdown_clone.cancel();
    });

    tokio::spawn(auth::start_watcher(tokens_path, shutdown.clone()));

    let (metrics_tx, metrics_rx) = tokio::sync::mpsc::channel::<MetricsEvent>(cfg.max_active_conns);

    let handler = handler::handler::Handler::new(cfg.clone(), metrics_tx.clone(), shutdown.clone());

    sleep(Duration::from_secs(1)).await;

    let codecs = get_codecs_with_retry(&handler, shutdown.clone()).await?;
    let _ = set_codecs(codecs)?;

    let metrics = Arc::new(Metrics::new());

    server::run(
        handler,
        cfg.listening_addr,
        cfg.http_port,
        cfg.https_port,
        cfg.http_enabled,
        cfg.https_enabled,
        cfg.auth_enabled,
        tls_cert_path,
        tls_key_path,
        shutdown,
        metrics_rx,
        metrics_tx,
        metrics,
    )
    .await
}

async fn get_codecs_with_retry(
    handler: &Handler,
    cancel_token: CancellationToken,
) -> Result<Codecs, RZError> {
    let mut attempt = 0;
    let max_attempts = 10;
    let delay_sec = 1;

    loop {
        if cancel_token.is_cancelled() {
            return Err(RZError::Cancelled);
        }

        match process_get_codecs(handler).await {
            Ok(codecs) => return Ok(codecs),
            Err(err) => {
                if !matches!(&err, RZError::NoFollowerNodeAvailable) {
                    return Err(err);
                }
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(err);
                }
                tracing::warn!("Roomzin cluster unavailable. Retrying ...");

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(delay_sec)) => {}
                    _ = cancel_token.cancelled() => {
                        return Err(RZError::Cancelled);
                    }
                }
            }
        }
    }
}
