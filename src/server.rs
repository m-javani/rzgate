// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use axum::Extension;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use futures::future::join_all;
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};

use axum::{Router, extract::State, routing::post};
use tracing::info;

use crate::auth::{self, AccessLevel};
use crate::metrics::{Metrics, MetricsEvent, apply_metric_event};
use crate::{error::RZError, handler::handler::Handler, processor::base::process};

struct AppState {
    handler: Arc<Handler>,
    metrics_tx: Sender<MetricsEvent>,
    auth_enabled: bool,
}

pub async fn run(
    handler: Arc<Handler>,
    listening_addr: String,
    http_port: u16,
    https_port: u16,
    http_enabled: bool,
    https_enabled: bool,
    auth_enabled: bool,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    cancel_token: CancellationToken,
    metrics_rx: Receiver<MetricsEvent>,
    metrics_tx: Sender<MetricsEvent>,
    node_metrics: Arc<Metrics>,
) -> Result<(), RZError> {
    // spawn background metrics updater
    let mut rx = metrics_rx;
    let metrics = node_metrics.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            apply_metric_event(&metrics, event);
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS]) // Added OPTIONS
        .allow_headers(Any);

    let mut handles: Vec<JoinHandle<_>> = Vec::new();
    if http_enabled {
        let http = tokio::spawn(http_server(
            listening_addr.clone(),
            http_port,
            handler.clone(),
            cancel_token.clone(),
            metrics_tx.clone(),
            node_metrics.clone(),
            cors.clone(),
            auth_enabled.clone(),
        ));
        handles.push(http);
    }

    if https_enabled {
        let https = tokio::spawn(https_server(
            listening_addr,
            https_port,
            handler,
            cancel_token,
            metrics_tx,
            node_metrics,
            cors,
            tls_cert,
            tls_key,
            auth_enabled,
        ));
        handles.push(https);
    }

    // Ignore errors.
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
    auth_enabled: bool,
) {
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
        .layer(middleware::from_fn_with_state(
            Arc::new(AppState {
                handler: handler.clone(),
                metrics_tx: metrics_tx.clone(),
                auth_enabled,
            }),
            auth_middleware,
        ))
        .with_state(Arc::new(AppState {
            handler,
            metrics_tx,
            auth_enabled,
        }));

    let handle = Handle::new();

    let address = format!("{}:{}", listening_addr, http_port);
    let addr: SocketAddr = address.parse().expect("Invalid https address");
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

    info!("api server listening on http://{}", address);

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

async fn https_server(
    listening_addr: String,
    https_port: u16,
    handler: Arc<Handler>,
    cancel_token: CancellationToken,
    metrics_tx: Sender<MetricsEvent>,
    node_metrics: Arc<Metrics>,
    cors: CorsLayer,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    auth_enabled: bool,
) {
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
        .layer(middleware::from_fn_with_state(
            Arc::new(AppState {
                handler: handler.clone(),
                metrics_tx: metrics_tx.clone(),
                auth_enabled,
            }),
            auth_middleware,
        ))
        .with_state(Arc::new(AppState {
            handler,
            metrics_tx,
            auth_enabled,
        }));

    let tls_config = RustlsConfig::from_pem_file(tls_cert.unwrap(), tls_key.unwrap())
        .await
        .map_err(|e| {
            tracing::error!("{}", RZError::Validation(e.to_string()).to_string());
        })
        .unwrap();

    let handle = Handle::new();

    let address = format!("{}:{}", listening_addr, https_port);
    let addr: SocketAddr = address.parse().expect("Invalid https address");
    let server = axum_server::bind_rustls(addr, tls_config)
        .handle(handle.clone())
        .serve(app.into_make_service());

    tokio::spawn({
        let shutdown = cancel_token.clone();
        async move {
            shutdown.cancelled().await;
            handle.graceful_shutdown(Some(Duration::from_secs(2)));
        }
    });

    info!("api server listening on https://{}", address);

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

async fn auth_middleware(
    State(state): State<Arc<AppState>>, // Keep if you need handler for other purposes
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    // Allow OPTIONS requests for CORS preflight
    if req.method() == Method::OPTIONS {
        return Ok(next.run(req).await);
    }

    if !state.auth_enabled {
        req.extensions_mut().insert(AccessLevel::Full);
        return Ok(next.run(req).await);
    }

    // Extract auth header
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing Authorization header".to_string(),
        ))?;

    // Require Bearer scheme
    if !auth_header.starts_with("Bearer ") {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid Authorization header scheme".to_string(),
        ));
    }

    // Extract token
    let token = auth_header.trim_start_matches("Bearer ").trim();

    // Use static auth function
    let access_level = auth::get_access_level(token).ok_or_else(|| {
        let _ = state
            .metrics_tx
            .try_send(MetricsEvent::ApiIncClientAuthFail);
        (
            StatusCode::UNAUTHORIZED,
            "Invalid or unauthorized token".to_string(),
        )
    })?;

    // Attach access level to request extensions
    req.extensions_mut().insert(access_level);

    Ok(next.run(req).await)
}

async fn process_request(
    State(state): State<Arc<AppState>>,
    Extension(access_level): Extension<AccessLevel>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    process(
        &body,
        &state.handler,
        state.metrics_tx.clone(),
        access_level,
    )
    .await
}
