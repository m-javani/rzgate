// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzproxy.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

pub mod bitmask;
pub mod config;
pub mod error;
pub mod handler;
pub mod helper;
pub mod metrics;
pub mod processor;
pub mod protocol;
pub mod server;

use crate::{
    bitmask::set_codecs,
    config::Config,
    error::RZError,
    handler::handler::Handler,
    metrics::{Metrics, MetricsRef},
    processor::get_codecs::process_get_codecs,
    protocol::Codecs,
};
use tokio_util::sync::CancellationToken;
use tracing::Level;
use tracing_subscriber::fmt::time::UtcTime;

use std::sync::Once;

static INIT: Once = Once::new();

pub fn init_logging(level: Level) {
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .compact()
            .with_timer(UtcTime::rfc_3339())
            .with_max_level(level)
            .with_target(false)
            .with_thread_names(false)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global tracing subscriber");
    });
}
pub async fn async_main(cfg: Config, shutdown: CancellationToken) -> Result<(), RZError> {
    init_logging(Level::INFO);

    // Create metrics once and pass it down
    let metrics: MetricsRef = Metrics::new();

    // Pass metrics to handler
    let handler = Handler::new(cfg.clone(), metrics.clone(), shutdown.clone());

    let codecs = get_codecs_with_retry(&handler, shutdown.clone()).await?;
    let _ = set_codecs(codecs)?;

    server::run(
        handler,
        metrics,
        cfg.listening_addr,
        cfg.http_port,
        shutdown,
    )
    .await
}

async fn get_codecs_with_retry(
    handler: &Handler,
    shutdown: CancellationToken,
) -> Result<Codecs, RZError> {
    let mut backoff = 1;
    let max_backoff = 5;
    let max_attempts = 60;

    for attempt in 0..max_attempts {
        tokio::select! {
            _ = shutdown.cancelled() => {
                return Err(RZError::Internal("Shutdown while waiting for codecs".into()));
            }
            result = process_get_codecs(handler) => {
                match result {
                    Ok(codecs) => {
                        tracing::info!("Successfully retrieved codecs on attempt {}", attempt + 1);
                        return Ok(codecs);
                    }
                    Err(e) => {
                        tracing::warn!(
                            attempt = attempt + 1,
                            error = %e,
                            "Failed to get codecs, retrying in {}s",
                            backoff
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        }
    }

    Err(RZError::Internal(
        "Max attempts reached waiting for codecs".into(),
    ))
}
