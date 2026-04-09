use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use sqlx::PgPool;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};


use crate::db;
use crate::geo::provider::{GeoProvider, GeocodeResult};
use crate::util::{cache::AddressCache, limiter::RateLimiter, metrics, retry::with_retry};

// ================================================================
// Config
// ================================================================

pub struct WorkerConfig {
    pub batch_size: usize,
    pub qps_limit: u32,
    pub max_retries: u32,
    pub scan_interval_secs: u64,
    pub dry_run: bool,
}

// ================================================================
// Worker
// ================================================================

pub struct GeocoderWorker {
    pool: Arc<PgPool>,
    providers: Vec<Box<dyn GeoProvider>>,
    limiter: RateLimiter,
    cache: AddressCache,
    config: WorkerConfig,
}

impl GeocoderWorker {
    pub fn new(
        pool: Arc<PgPool>,
        providers: Vec<Box<dyn GeoProvider>>,
        config: WorkerConfig,
    ) -> Self {
        let limiter = RateLimiter::new(config.qps_limit);
        Self {
            pool,
            providers,
            limiter,
            cache: AddressCache::new(),
            config,
        }
    }

    // ------------------------------------------------------------
    // Main loop
    // ------------------------------------------------------------

    pub async fn run(&self) -> Result<()> {
        info!("Worker started, entering main loop");

        // Initial backfill
        self.scan_and_enqueue().await?;

        loop {
            match self.process_batch().await {
                Ok(0) => {
                    // Queue empty — scan for new drives then sleep
                    self.scan_and_enqueue().await?;
                    debug!(interval = self.config.scan_interval_secs, "Sleeping");
                    sleep(Duration::from_secs(self.config.scan_interval_secs)).await;
                }
                Ok(count) => {
                    debug!(count, "Batch processed");
                }
                Err(e) => {
                    error!(error = %e, "Batch error, backing off");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    // ------------------------------------------------------------
    // Scan drives missing addresses → enqueue
    // ------------------------------------------------------------

    pub async fn scan_and_enqueue(&self) -> Result<()> {
        let missing = db::scan_missing_addresses(&self.pool).await?;
        if missing.is_empty() {
            debug!("No missing addresses found");
            return Ok(());
        }

        info!(count = missing.len(), "Found drives with missing addresses");
        let inserted = db::enqueue_items(&self.pool, &missing).await?;
        info!(inserted, "Enqueued geocode items");
        Ok(())
    }

    // ------------------------------------------------------------
    // Process one batch
    // ------------------------------------------------------------

    async fn process_batch(&self) -> Result<usize> {
        let items = db::fetch_batch(&self.pool, self.config.batch_size as i64).await?;
        if items.is_empty() {

            debug!("Queue is empty on fetch");
            return Ok(0);
        }

        info!(count = items.len(), "Processing batch");

        for item in &items {
            // Check for duplicates already processed in this batch
            if let Some(cached_id) = self.cache.get(item.latitude, item.longitude).await {
                debug!(id = item.id, address_id = cached_id, "Cache hit");
                if !self.config.dry_run {
                    db::update_drive_address(&self.pool, item.drive_id, &item.address_type, cached_id).await?;
                    db::mark_done(&self.pool, item.id).await?;
                }
                continue;
            }

            // Check DB cache
            if let Some(existing_id) = db::find_address_by_coord(&self.pool, item.latitude, item.longitude).await? {
                debug!(id = item.id, address_id = existing_id, "DB cache hit");
                self.cache.insert(item.latitude, item.longitude, existing_id).await;
                if !self.config.dry_run {
                    db::update_drive_address(&self.pool, item.drive_id, &item.address_type, existing_id).await?;
                    db::mark_done(&self.pool, item.id).await?;
                }
                continue;
            }

            // Geocode with retry + fallback
            match self.geocode_with_retry(&item).await {
                Ok(address_id) => {
                    metrics::record_success();
                    self.cache.insert(item.latitude, item.longitude, address_id).await;
                    if !self.config.dry_run {
                        db::update_drive_address(&self.pool, item.drive_id, &item.address_type, address_id).await?;
                        db::mark_done(&self.pool, item.id).await?;
                    }
                    info!(
                        id = item.id,
                        drive = item.drive_id,
                        address_id,
                        lat = item.latitude,
                        lng = item.longitude,
                        "Geocoded"
                    );
                }
                Err(e) => {
                    metrics::record_failure();
                    error!(id = item.id, error = %e, "Geocode failed permanently");
                    if !self.config.dry_run {
                        db::mark_dead(&self.pool, item.id, &format!("{}", e)).await?;
                    }
                }
            }
        }

        // Update charging_processes addresses once per batch (not per item)
        if !self.config.dry_run {
            if let Err(e) = db::update_charge_address(&self.pool).await {
                warn!(error = %e, "Failed to update charge addresses");
            }
        }

        Ok(items.len())
    }

    // ------------------------------------------------------------
    // Geocode with retry across provider chain
    // ------------------------------------------------------------

    async fn geocode_with_retry(&self, item: &db::QueueItem) -> Result<i32> {
        let result = with_retry(self.config.max_retries, 1000, || {
            self.geocode_with_fallback(item.latitude, item.longitude)
        })
        .await?;

        if self.config.dry_run {
            return Ok(-1);
        }

        let address_id = db::insert_address(
            &self.pool,
            item.latitude,
            item.longitude,
            result.display_name.as_ref(),
            result.city.as_deref(),
            result.province.as_deref(),
            result.country.as_deref(),
            result.postcode.as_deref(),
            result.name.as_deref(),
            result.house_number.as_deref(),
            result.road.as_deref(),
            result.neighbourhood.as_deref(),
            result.state_district.as_deref(),
            result.raw.clone(),
        )
        .await?;

        Ok(address_id)
    }

    // ------------------------------------------------------------
    // Try each provider in order (fallback chain)
    // ------------------------------------------------------------

    async fn geocode_with_fallback(&self, lat: f64, lng: f64) -> Result<GeocodeResult> {
        let mut last_err: Option<anyhow::Error> = None;

        for provider in &self.providers {
            self.limiter.acquire().await;

            let start = Instant::now();
            match provider.reverse_geocode(lat, lng).await {
                Ok(result) => {
                    let elapsed = start.elapsed().as_secs_f64();
                    metrics::record_latency(provider.name(), elapsed);
                    debug!(
                        provider = provider.name(),
                        latency_ms = (elapsed * 1000.0) as u64,
                        "Provider succeeded"
                    );
                    return Ok(result);
                }
                Err(e) => {
                    let elapsed = start.elapsed().as_secs_f64();
                    metrics::record_latency(provider.name(), elapsed);
                    warn!(
                        provider = provider.name(),
                        error = %e,
                        "Provider failed, trying next"
                    );
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("all providers failed")))
    }
}
