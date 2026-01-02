// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

// handler.rs
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::error::RZError;
use crate::handler::cluster_handler::ClusterHandler;
use crate::handler::no_cluster_handler::NoClusterHandler;
use crate::metrics::MetricsEvent;

#[derive(Clone)]
pub struct Handler {
    inner: Arc<HandlerInner>,
}

struct HandlerInner {
    // Only one of these will be Some
    cluster: Option<Arc<ClusterHandler>>,
    no_cluster: Option<Arc<NoClusterHandler>>,
}

impl Handler {
    pub fn new(
        cfg: Config,
        metrics_tx: Sender<MetricsEvent>,
        cancel_token: CancellationToken,
    ) -> Arc<Self> {
        let inner = if cfg.no_cluster {
            let no_cluster = NoClusterHandler::new(cfg, metrics_tx, cancel_token);
            HandlerInner {
                cluster: None,
                no_cluster: Some(no_cluster),
            }
        } else {
            let cluster = ClusterHandler::new(cfg, metrics_tx, cancel_token);
            HandlerInner {
                cluster: Some(cluster),
                no_cluster: None,
            }
        };

        Arc::new(Self {
            inner: Arc::new(inner),
        })
    }

    /// Execute a request — writes and reads are handled according to the mode
    pub async fn execute(&self, is_write: bool, payload: Vec<u8>) -> Result<Bytes, RZError> {
        if let Some(ref nc) = self.inner.no_cluster {
            nc.execute(is_write, payload).await
        } else if let Some(ref cluster) = self.inner.cluster {
            cluster.execute(is_write, payload).await
        } else {
            Err(RZError::Validation(
                "Handler not properly initialized".into(),
            ))
        }
    }
}
