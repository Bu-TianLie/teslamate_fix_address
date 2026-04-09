use anyhow::{Context, Result};

pub struct AppConfig {
    pub database_url: String,
    pub tencent_map_key: Option<String>,
    pub amap_key: Option<String>,
    pub baidu_ak: Option<String>,
    pub provider_order: Vec<String>,
    pub max_retries: u32,
    pub scan_interval_secs: u64,
    pub db_max_connections: u32,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL is required")?;

        let tencent_map_key = std::env::var("TENCENT_MAP_KEY").ok().filter(|s| !s.is_empty());
        let amap_key = std::env::var("AMAP_KEY").ok().filter(|s| !s.is_empty());
        let baidu_ak = std::env::var("BAIDU_AK").ok().filter(|s| !s.is_empty());

        let provider_order = std::env::var("PROVIDER_ORDER")
            .unwrap_or_else(|_| "tencent,amap,baidu".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let max_retries = std::env::var("MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        let scan_interval_secs = std::env::var("SCAN_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let db_max_connections = std::env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        Ok(Self {
            database_url,
            tencent_map_key,
            amap_key,
            baidu_ak,
            provider_order,
            max_retries,
            scan_interval_secs,
            db_max_connections,
        })
    }
}
