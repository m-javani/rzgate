use rzgate::{
    config::Config,
    error::RZError,
    handler::handler::Handler,
    metrics::{Metrics, MetricsEvent},
    server,
};
use std::sync::Arc;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::Level;
use tracing_subscriber::fmt::time::UtcTime;

fn main() -> Result<(), RZError> {
    // Setup tracing
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_timer(UtcTime::rfc_3339())
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_names(false)
        .with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");

    let cfg = Config::parse();
    let desired_workers = cfg.effective_workers();

    tracing::info!(
        mode = %cfg.mode,
        listen = %cfg.listening_addr,
        port = %cfg.http_port,
        cluster = %cfg.roomzin_addr,
        cluster_port = %cfg.roomzin_port,
        workers = %desired_workers,
        "Starting RZGate"
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(desired_workers)
        .max_blocking_threads(512)
        .enable_all()
        .build()
        .map_err(|e| RZError::Internal(format!("Failed to build runtime: {e}")))?;

    rt.block_on(async_main(cfg))
}

async fn async_main(cfg: Config) -> Result<(), RZError> {
    let shutdown = CancellationToken::new();

    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        tracing::info!("Ctrl+C received");
        shutdown_clone.cancel();
    });

    let (metrics_tx, metrics_rx) = tokio::sync::mpsc::channel::<MetricsEvent>(cfg.max_active_conns);

    let handler = Handler::new(cfg.clone(), metrics_tx.clone(), shutdown.clone());
    sleep(tokio::time::Duration::from_secs(1)).await;

    let metrics = Arc::new(Metrics::new());

    server::run(
        handler,
        cfg.listening_addr,
        cfg.http_port,
        shutdown,
        metrics_rx,
        metrics_tx,
        metrics,
    )
    .await
}
