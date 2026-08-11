// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use axum::http::{Method, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum_server::Handle;
use futures::future::join_all;
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};

use axum::{Router, extract::State, routing::post};
use tracing::info;

use crate::metrics::{Metrics, MetricsEvent, apply_metric_event};
use crate::{error::RZError, handler::handler::Handler, processor::base::process};

struct AppState {
    handler: Arc<Handler>,
    metrics_tx: Sender<MetricsEvent>,
}

pub async fn run(
    handler: Arc<Handler>,
    listening_addr: String,
    http_port: u16,
    cancel_token: CancellationToken,
    metrics_rx: Receiver<MetricsEvent>,
    metrics_tx: Sender<MetricsEvent>,
    node_metrics: Arc<Metrics>,
) -> Result<(), RZError> {
    // Spawn background metrics updater
    let mut rx = metrics_rx;
    let metrics = node_metrics.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            apply_metric_event(&metrics, event);
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let mut handles: Vec<JoinHandle<_>> = Vec::new();

    let http = tokio::spawn(http_server(
        listening_addr,
        http_port,
        handler,
        cancel_token,
        metrics_tx,
        node_metrics,
        cors,
    ));
    handles.push(http);

    // Ignore errors
    let _ = join_all(handles).await;

    Ok(())
}

async fn http_server(
    listening_addr: String,
    http_port: u16,
    handler: Arc<Handler>,
    cancel_token: CancellationToken,
    metrics_tx: Sender<MetricsEvent>,
    node_metrics: Arc<Metrics>,
    cors: CorsLayer,
) {
    let state = Arc::new(AppState {
        handler: handler.clone(),
        metrics_tx: metrics_tx.clone(),
    });

    let app = Router::new()
        .route("/api", post(process_request))
        .route(
            "/metrics",
            get(move || async move {
                let raw_output = node_metrics.prometheus_handle.render();
                (StatusCode::OK, raw_output)
            }),
        )
        .layer(cors)
        .with_state(state);

    let handle = Handle::new();

    let address = format!("{}:{}", listening_addr, http_port);
    let addr: SocketAddr = address.parse().expect("Invalid address");
    let server = axum_server::bind(addr)
        .handle(handle.clone())
        .serve(app.into_make_service());

    tokio::spawn({
        let shutdown = cancel_token.clone();
        async move {
            shutdown.cancelled().await;
            handle.graceful_shutdown(Some(Duration::from_secs(2)));
        }
    });

    info!("RzGate listening on http://{}", address);

    server
        .await
        .map_err(|e| {
            tracing::debug!("error in axum api server {:?}", e);
            tracing::error!(
                "{}",
                RZError::System("api server crashed".into()).to_string()
            );
        })
        .unwrap();
}

async fn process_request(State(state): State<Arc<AppState>>, body: axum::body::Bytes) -> Response {
    // Always pass Full access (no auth)
    process(&body, &state.handler, state.metrics_tx.clone()).await
}
