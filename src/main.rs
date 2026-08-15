use rzgate::{
    bitmask::set_codecs,
    config::Config,
    error::RZError,
    handler::handler::Handler,
    metrics::{Metrics, MetricsRef},
    processor::get_codecs::process_get_codecs,
    protocol::Codecs,
    server,
};
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

    // Create metrics once and pass it down
    let metrics: MetricsRef = Metrics::new();

    // Pass metrics to handler
    let handler = Handler::new(cfg.clone(), metrics.clone(), shutdown.clone());

    let codecs = get_codecs_with_retry(&handler, shutdown.clone()).await?;
    let _ = set_codecs(codecs)?;

    server::run(
        handler,
        metrics,
        cfg.listening_addr,
        cfg.http_port,
        shutdown,
    )
    .await
}

async fn get_codecs_with_retry(
    handler: &Handler,
    shutdown: CancellationToken,
) -> Result<Codecs, RZError> {
    let mut backoff = 1;
    let max_backoff = 5;
    let max_attempts = 60;

    for attempt in 0..max_attempts {
        tokio::select! {
            _ = shutdown.cancelled() => {
                return Err(RZError::Internal("Shutdown while waiting for codecs".into()));
            }
            result = process_get_codecs(handler) => {
                match result {
                    Ok(codecs) => {
                        tracing::info!("Successfully retrieved codecs on attempt {}", attempt + 1);
                        return Ok(codecs);
                    }
                    Err(e) => {
                        tracing::warn!(
                            attempt = attempt + 1,
                            error = %e,
                            "Failed to get codecs, retrying in {}s",
                            backoff
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        }
    }

    Err(RZError::Internal(
        "Max attempts reached waiting for codecs".into(),
    ))
}
