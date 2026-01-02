// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

// no_cluster_handler.rs
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::error::RZError;
use crate::handler::connection::Connection;
use crate::handler::demux::DemuxMap;
use crate::metrics::MetricsEvent;

#[derive(Clone)]
pub struct NoClusterHandler {
    inner: Arc<NoClusterHandlerInner>,
}

struct NoClusterHandlerInner {
    cfg: Config,
    roomzin_tcp_host: String,
    connections: Arc<RwLock<Vec<Option<Connection>>>>,
    metrics_tx: Sender<MetricsEvent>,
    cancel_token: CancellationToken,
    // Simple round-robin
    next_conn: Mutex<usize>,
}

impl NoClusterHandler {
    pub fn new(
        cfg: Config,
        metrics_tx: Sender<MetricsEvent>,
        cancel_token: CancellationToken,
    ) -> Arc<Self> {
        let conn_count = cfg.conn_per_roomzin_node;
        let conns = vec![None; conn_count];

        // Initial connection establishment
        let host = cfg
            .roomzin_standalone_host
            .as_ref()
            .expect("standalone addr missing");
        let handler = Arc::new(Self {
            inner: Arc::new(NoClusterHandlerInner {
                cfg: cfg.clone(),
                roomzin_tcp_host: host.clone(),
                connections: Arc::new(RwLock::new(conns)),
                metrics_tx: metrics_tx.clone(),
                cancel_token: cancel_token.clone(),
                next_conn: Mutex::new(0),
            }),
        });

        let h_clone = handler.clone();
        tokio::spawn(async move { h_clone.reconnect_closed().await });

        // Spawn background connection maintainer
        let h = handler.clone();
        tokio::spawn(async move { h.maintain_connections().await });

        handler
    }

    async fn maintain_connections(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = self.inner.cancel_token.cancelled() => break,
                _ = interval.tick() => {
                    self.reconnect_closed().await;
                }
            }
        }
    }

    async fn reconnect_closed(&self) {
        let mut conns = self.inner.connections.write().await;
        let addr = self.inner.roomzin_tcp_host.clone();

        for (_i, slot) in conns.iter_mut().enumerate() {
            let should_connect = match slot {
                Some(c) => c.is_closed(),
                None => true,
            };

            if should_connect {
                match Connection::connect(addr.clone(), &self.inner.cfg, DemuxMap::new()).await {
                    Ok(conn) => {
                        *slot = Some(conn);
                        let _ = self
                            .inner
                            .metrics_tx
                            .try_send(MetricsEvent::BackendOpenedConnections);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to connect to {} : {}", addr, e);
                    }
                }
            }
        }
    }

    async fn next_connection(&self) -> Option<Connection> {
        let mut idx = self.inner.next_conn.lock().await;
        let conns = self.inner.connections.read().await;

        for _ in 0..conns.len() {
            let i = *idx;
            *idx = (*idx + 1) % conns.len();

            if let Some(conn) = &conns[i] {
                if !conn.is_closed() {
                    return Some(conn.clone());
                }
            }
        }
        None
    }

    pub async fn execute(&self, _is_write: bool, payload: Vec<u8>) -> Result<Bytes, RZError> {
        if payload.is_empty() {
            return Err(RZError::Validation("empty payload".into()));
        }

        let mut attempts = 0;
        loop {
            let conn = match self.next_connection().await {
                Some(c) if !c.is_closed() => c,
                _ => {
                    attempts += 1;
                    if attempts >= 3 {
                        return Err(RZError::RoomzinUnreachable(
                            self.inner.roomzin_tcp_host.clone(),
                        )); // reuse existing error or make new one
                    }
                    sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                    continue;
                }
            };

            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            let corr_id = conn.next_corr_id();

            conn.inner
                .demux
                .store(corr_id, resp_tx, std::time::Instant::now())
                .await;
            let _ = conn.send(corr_id, payload.clone()).await;

            match resp_rx.await {
                Ok(response_bytes) => return Ok(response_bytes),
                Err(_) => {
                    attempts += 1;
                    if attempts >= 3 {
                        return Err(RZError::Timeout);
                    }
                    sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                }
            }
        }
    }
}
