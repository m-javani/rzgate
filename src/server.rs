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
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};

use axum::{Router, extract::State, routing::post};
use tracing::info;

use crate::metrics::MetricsRef;
use crate::{error::RZError, handler::handler::Handler, processor::base::process};

struct AppState {
    handler: Arc<Handler>,
    metrics: MetricsRef,
}

pub async fn run(
    handler: Arc<Handler>,
    metrics: MetricsRef,
    listening_addr: String,
    http_port: u16,
    cancel_token: CancellationToken,
) -> Result<(), RZError> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    http_server(
        listening_addr,
        http_port,
        handler,
        metrics,
        cancel_token,
        cors,
    )
    .await;

    Ok(())
}

async fn http_server(
    listening_addr: String,
    http_port: u16,
    handler: Arc<Handler>,
    metrics: MetricsRef,
    cancel_token: CancellationToken,
    cors: CorsLayer,
) {
    let state = Arc::new(AppState {
        handler: handler.clone(),
        metrics: metrics.clone(),
    });

    let app = Router::new()
        .route("/api", post(process_request))
        .route("/health", get(health_handler))
        .route(
            "/metrics",
            get(move || async move {
                let raw_output = metrics.prometheus_handle.render();
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
                RZError::Internal("api server crashed".into()).to_string()
            );
        })
        .unwrap();
}

async fn process_request(State(state): State<Arc<AppState>>, body: axum::body::Bytes) -> Response {
    process(&body, &state.handler, state.metrics.clone()).await
}

async fn health_handler() -> &'static str {
    "OK"
}
