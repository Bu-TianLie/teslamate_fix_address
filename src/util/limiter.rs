use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

/// Token-bucket style rate limiter.
/// Enforces a minimum interval between acquisitions.
pub struct RateLimiter {
    last: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl RateLimiter {
    pub fn new(qps: u32) -> Self {
        assert!(qps > 0, "QPS must be > 0");
        Self {
            last: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1))),
            min_interval: Duration::from_secs_f64(1.0 / qps as f64),
        }
    }

    /// Block until a token is available, then acquire it.
    pub async fn acquire(&self) {
        let mut last = self.last.lock().await;
        let now = Instant::now();
        let next = *last + self.min_interval;
        if now < next {
            let wait = next - now;
            drop(last);
            sleep(wait).await;
            *self.last.lock().await = Instant::now();
        } else {
            *last = now;
        }
    }
}
