// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

// unified_handler.rs
use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, Mode};
use crate::error::RZError;
use crate::handler::connection::Connection;
use crate::handler::demux::DemuxMap;
use crate::metrics::MetricsEvent;
use crate::processor::base::{prepend_header, prepend_router_header};

pub struct Handler {
    inner: Arc<HandlerInner>,
}

struct HandlerInner {
    cfg: Config,
    target_addr: String,
    target_port: u16,
    mode: Mode,
    connections: Arc<RwLock<Vec<Option<Connection>>>>,
    #[allow(unused)]
    metrics_tx: Sender<MetricsEvent>,
    cancel_token: CancellationToken,
    next_conn: Mutex<usize>,
}

impl Handler {
    pub fn new(
        cfg: Config,
        metrics_tx: Sender<MetricsEvent>,
        cancel_token: CancellationToken,
    ) -> Arc<Self> {
        let conn_count = cfg.conn_per_node;
        let conns = vec![None; conn_count];

        // Determine target based on mode
        let (target_addr, target_port, mode) = match cfg.mode {
            Mode::Standalone => (cfg.roomzin_addr.clone(), cfg.roomzin_port, Mode::Standalone),
            Mode::Router => (cfg.roomzin_addr.clone(), cfg.roomzin_port, Mode::Router),
        };

        let handler = Arc::new(Self {
            inner: Arc::new(HandlerInner {
                cfg: cfg.clone(),
                target_addr,
                target_port,
                mode,
                connections: Arc::new(RwLock::new(conns)),
                metrics_tx: metrics_tx.clone(),
                cancel_token: cancel_token.clone(),
                next_conn: Mutex::new(0),
            }),
        });

        let h_clone = handler.clone();
        tokio::spawn(async move { h_clone.reconnect_closed().await });

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
        let addr = self.inner.target_addr.clone();
        let port = self.inner.target_port;

        for slot in conns.iter_mut() {
            let should_connect = match slot {
                Some(c) => c.is_closed(),
                None => true,
            };

            if should_connect {
                let addr_with_port = format!("{}:{}", addr, port);
                match Connection::connect(addr_with_port, &self.inner.cfg, DemuxMap::new()).await {
                    Ok(conn) => {
                        *slot = Some(conn);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to connect to {}:{} : {}", addr, port, e);
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

    pub async fn execute(
        &self,
        segment: &str,
        is_write: bool,
        payload: Vec<u8>,
    ) -> Result<Bytes, RZError> {
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
                        return Err(RZError::RoomzinUnreachable(self.inner.target_addr.clone()));
                    }
                    sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                    continue;
                }
            };

            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            let corr_id = conn.next_corr_id();

            // Build the frame based on mode
            let frame = match self.inner.mode {
                Mode::Standalone => prepend_header(corr_id, &payload),
                Mode::Router => prepend_router_header(segment, is_write, corr_id, &payload),
            };

            conn.inner
                .demux
                .store(corr_id, resp_tx, std::time::Instant::now())
                .await;

            // Send the frame
            let _ = conn.send_frame(frame).await;

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
