// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use metrics::{Counter, Gauge, counter, gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

#[derive(Debug)]
pub struct ApiMetrics {
    commands: Counter,
    bytes_received: Counter,
    bytes_sent: Counter,
    client_errors: Counter,
    client_login_fail: Counter,
}

#[derive(Debug)]
pub struct BackendMetrics {
    followers: Gauge,
    leader_change: Counter,
    opened_connections: Counter,
    disconnects: Counter,
}

#[derive(Debug, Clone, Copy)]
pub enum MetricsEvent {
    // ------------- Api -------------
    ApiIncCommands,
    ApiAddBytesReceived(u64),
    ApiAddBytesSent(u64),
    ApiIncClientErrors,
    ApiIncClientAuthFail,

    // ------------- Api -------------
    BackendSetFollowers(u32),
    BackendIncleaderChange,
    BackendOpenedConnections,
    BackendIncDisconnects,
}

impl ApiMetrics {
    pub fn new() -> Self {
        ApiMetrics {
            commands: counter!("api_commands_total"),
            bytes_received: counter!("api_bytes_received_total"),
            bytes_sent: counter!("api_bytes_sent_total"),
            client_errors: counter!("api_client_errors_total"),
            client_login_fail: counter!("api_client_login_fail_total"),
        }
    }

    pub fn inc_commands(&self) {
        self.commands.increment(1);
    }

    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.increment(bytes);
    }

    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.increment(bytes);
    }

    pub fn inc_client_errors(&self) {
        self.client_errors.increment(1);
    }

    pub fn inc_client_login_fail(&self) {
        self.client_login_fail.increment(1);
    }
}

impl BackendMetrics {
    pub fn new() -> Self {
        BackendMetrics {
            followers: gauge!("backend_followers_total"),
            leader_change: counter!("backend_leader_change_total"),
            opened_connections: counter!("backend_opened_connections_total"),
            disconnects: counter!("backend_disconnect_total"),
        }
    }

    pub fn set_backend_followers(&self, count: u32) {
        self.followers.set(count as f64);
    }

    pub fn inc_backend_leader_change(&self) {
        self.leader_change.increment(1);
    }

    pub fn inc_backend_disconnect(&self) {
        self.disconnects.increment(1);
    }

    pub fn inc_backend_connections(&self) {
        self.opened_connections.increment(1);
    }
}

pub struct Metrics {
    pub api: ApiMetrics,
    pub backend: BackendMetrics,
    pub prometheus_handle: PrometheusHandle,
}

impl Metrics {
    pub fn new() -> Metrics {
        let prometheus_handle = PrometheusBuilder::new()
            .install_recorder()
            .expect("Failed to install Prometheus recorder");

        Metrics {
            api: ApiMetrics::new(),
            backend: BackendMetrics::new(),
            prometheus_handle,
        }
    }
}

pub fn apply_metric_event(metrics: &Metrics, event: MetricsEvent) {
    use MetricsEvent::*;

    match event {
        ApiIncCommands => metrics.api.inc_commands(),
        ApiAddBytesReceived(n) => metrics.api.add_bytes_received(n),
        ApiAddBytesSent(n) => metrics.api.add_bytes_sent(n),
        ApiIncClientErrors => metrics.api.inc_client_errors(),
        ApiIncClientAuthFail => metrics.api.inc_client_login_fail(),

        BackendSetFollowers(n) => metrics.backend.set_backend_followers(n),
        BackendIncleaderChange => metrics.backend.inc_backend_leader_change(),
        BackendOpenedConnections => metrics.backend.inc_backend_connections(),
        BackendIncDisconnects => metrics.backend.inc_backend_disconnect(),
    }
}
