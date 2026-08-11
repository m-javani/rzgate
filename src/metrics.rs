// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use metrics::{Counter, counter};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

#[derive(Debug)]
pub struct ApiMetrics {
    commands: Counter,
    bytes_received: Counter,
    bytes_sent: Counter,
    client_errors: Counter,
}

#[derive(Debug, Clone, Copy)]
pub enum MetricsEvent {
    ApiIncCommands,
    ApiAddBytesReceived(u64),
    ApiAddBytesSent(u64),
    ApiIncClientErrors,
}

impl ApiMetrics {
    pub fn new() -> Self {
        ApiMetrics {
            commands: counter!("api_commands_total"),
            bytes_received: counter!("api_bytes_received_total"),
            bytes_sent: counter!("api_bytes_sent_total"),
            client_errors: counter!("api_client_errors_total"),
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
}

pub struct Metrics {
    pub api: ApiMetrics,
    pub prometheus_handle: PrometheusHandle,
}

impl Metrics {
    pub fn new() -> Metrics {
        let prometheus_handle = PrometheusBuilder::new()
            .install_recorder()
            .expect("Failed to install Prometheus recorder");

        Metrics {
            api: ApiMetrics::new(),
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
    }
}
