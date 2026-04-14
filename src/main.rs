mod config;
mod db;
mod geo;
mod util;
mod worker;


use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use config::AppConfig;
use geo::provider::GeoProvider;
use worker::GeocoderWorker;

#[derive(Parser, Debug)]
#[command(name = "teslamate-geocoder", version, about = "TeslaMate geocoding worker for China")]
struct Cli {
    /// Number of queue items per batch
    #[arg(long, default_value_t = 10)]
    batch_size: usize,

    /// Requests per second limit (per provider)
    #[arg(long, default_value_t = 3)]
    qps: u32,

    /// Preferred provider (tencent / amap / baidu)
    #[arg(long)]
    provider: Option<String>,

    /// Dry-run mode: log but do not write
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Enable Prometheus metrics endpoint
    #[arg(long, default_value_t = false)]
    metrics: bool,

    /// Metrics listen address
    #[arg(long, default_value = "127.0.0.1:9090")]
    metrics_addr: String,

    /// Run backfill once and exit
    #[arg(long, default_value_t = false)]
    backfill: bool,

    /// GeoHash cluster precision (1..12). 7 ≈ 153m, 8 ≈ 38m
    #[arg(long, default_value_t = 7)]
    cluster_precision: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "teslamate_geocoder=info".into()),
        )
        .with_file(true)
        .with_line_number(true)
        .init();

    let cli = Cli::parse();
    let config = AppConfig::from_env()?;

    info!("TeslaMate Geocoder starting");
    info!(
        providers = %config.provider_order.join(", "),
        batch_size = cli.batch_size,
        qps = cli.qps,
        dry_run = cli.dry_run,
    );

    // ---- Database ----
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await?;

    sqlx::query("SELECT 1").execute(&pool).await?;
    info!("Database connected");

    db::ensure_schema(&pool).await?;

    // ---- Providers ----
    let providers = resolve_providers(&config, &cli)?;
    if providers.is_empty() {
        anyhow::bail!("No geocoding provider available. Configure at least one API key.");
    }
    info!(
        count = providers.len(),
        names = ?providers.iter().map(|p| p.name()).collect::<Vec<_>>(),
        "Providers resolved"
    );

    // ---- Metrics server ----
    if cli.metrics {
        let addr: std::net::SocketAddr = cli.metrics_addr.parse()?;
        tokio::spawn(async move {
            if let Err(e) = util::metrics::serve_metrics(addr).await {
                error!(error = %e, "Metrics server failed");
            }
        });
        info!(%addr, "Metrics server started");
    }

    // ---- Worker ----
    let worker = GeocoderWorker::new(
        Arc::new(pool),
        providers,
        worker::WorkerConfig {
            batch_size: cli.batch_size,
            qps_limit: cli.qps,
            max_retries: config.max_retries,
            scan_interval_secs: config.scan_interval_secs,
            dry_run: cli.dry_run,
            cluster_precision: cli.cluster_precision,
        },
    );

    if cli.backfill {
        info!("Running one-time backfill...");
        worker.run_backfill().await?;
        info!("Backfill complete");
        return Ok(());
    }

    // ---- Graceful shutdown ----
    let shutdown = tokio::signal::ctrl_c();

    tokio::select! {
        result = worker.run() => {
            if let Err(ref e) = result {
                error!(error = %e, "Worker exited with error");
            }
            result
        }
        _ = shutdown => {
            info!("Shutdown signal received, exiting");
            Ok(())
        }
    }
}

fn resolve_providers(config: &AppConfig, cli: &Cli) -> Result<Vec<Box<dyn GeoProvider>>> {
    let mut providers: Vec<Box<dyn GeoProvider>> = Vec::new();

    let order: Vec<&str> = if let Some(ref p) = cli.provider {
        vec![p.as_str()]
    } else {
        config.provider_order.iter().map(|s| s.as_str()).collect()
    };

    for name in &order {
        match *name {
            "tencent" => {
                if let Some(ref key) = config.tencent_map_key {
                    providers.push(Box::new(geo::tencent::TencentProvider::new(key.clone())));
                } else {
                    info!("Skipping tencent: no TENCENT_MAP_KEY");
                }
            }
            "amap" => {
                if let Some(ref key) = config.amap_key {
                    providers.push(Box::new(geo::amap::AmapProvider::new(key.clone())));
                } else {
                    info!("Skipping amap: no AMAP_KEY");
                }
            }
            "baidu" => {
                if let Some(ref ak) = config.baidu_ak {
                    providers.push(Box::new(geo::baidu::BaiduProvider::new(ak.clone())));
                } else {
                    info!("Skipping baidu: no BAIDU_AK");
                }
            }
            other => error!(name = other, "Unknown provider name"),
        }
    }

    Ok(providers)
}
