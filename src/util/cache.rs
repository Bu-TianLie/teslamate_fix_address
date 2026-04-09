use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_CACHE_SIZE: usize = 100_000;

/// In-memory cache: (lat_key, lng_key) → address_id.
/// Uses rounded coordinates (6 decimal places) as keys,
/// matching PostgreSQL numeric(8,6) precision ≈ 0.11 m.
/// Clears automatically when it exceeds MAX_CACHE_SIZE entries.
#[derive(Clone)]
pub struct AddressCache {
    inner: Arc<RwLock<HashMap<(i64, i64), i32>>>,
}

impl AddressCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::with_capacity(4096))),
        }
    }

    /// Round to 6 decimal places to match numeric(8,6).
    fn key(lat: f64, lng: f64) -> (i64, i64) {
        let lat_rounded = (lat * 1_000_000.0).round() as i64;
        let lng_rounded = (lng * 1_000_000.0).round() as i64;
        (lat_rounded, lng_rounded)
    }

    pub async fn get(&self, lat: f64, lng: f64) -> Option<i32> {
        self.inner.read().await.get(&Self::key(lat, lng)).copied()
    }

    pub async fn insert(&self, lat: f64, lng: f64, address_id: i32) {
        let mut map = self.inner.write().await;
        if map.len() >= MAX_CACHE_SIZE {
            map.clear();
        }
        map.insert(Self::key(lat, lng), address_id);
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}
