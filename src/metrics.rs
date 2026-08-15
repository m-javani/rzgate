use metrics::{Counter, Gauge, counter, gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::Arc;

pub type MetricsRef = Arc<Metrics>;

#[derive(Clone)]
pub struct Metrics {
    pub prometheus_handle: PrometheusHandle,
    commands: Counter,
    bytes_received: Counter,
    bytes_sent: Counter,
    client_errors: Counter,
    backend_timeouts: Counter,
    backend_connections: Gauge,
}

impl Metrics {
    pub fn new() -> MetricsRef {
        let prometheus_handle = PrometheusBuilder::new()
            .install_recorder()
            .expect("Failed to install Prometheus recorder");

        Arc::new(Metrics {
            prometheus_handle,
            commands: counter!("api_commands_total"),
            bytes_received: counter!("api_bytes_received_total"),
            bytes_sent: counter!("api_bytes_sent_total"),
            client_errors: counter!("api_client_errors_total"),
            backend_timeouts: counter!("backend_timeouts_total"),
            backend_connections: gauge!("backend_connections_current"),
        })
    }

    #[inline]
    pub fn inc_backend_timeouts(&self) {
        self.backend_timeouts.increment(1);
    }

    #[inline]
    pub fn inc_backend_connections(&self) {
        self.backend_connections.increment(1.0);
    }

    #[inline]
    pub fn dec_backend_connections(&self) {
        self.backend_connections.decrement(1.0);
    }

    #[inline]
    pub fn inc_commands(&self) {
        self.commands.increment(1);
    }

    #[inline]
    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.increment(bytes);
    }

    #[inline]
    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.increment(bytes);
    }

    #[inline]
    pub fn inc_client_errors(&self) {
        self.client_errors.increment(1);
    }
}
