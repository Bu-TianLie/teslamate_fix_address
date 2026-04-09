use anyhow::Result;
use std::future::Future;
use tokio::time::{sleep, Duration};
use tracing::warn;

/// Execute an async closure with exponential-backoff retries.
///
/// `base_delay_ms` is doubled on each successive retry.
pub async fn with_retry<F, Fut, T>(
    max_attempts: u32,
    base_delay_ms: u64,
    mut action: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last_err = None;

    for attempt in 0..max_attempts {
        match action().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                warn!(
                    attempt = attempt + 1,
                    max = max_attempts,
                    error = %e,
                    "Retryable error"
                );
                last_err = Some(e);
                if attempt + 1 < max_attempts {
                    let delay = base_delay_ms * 2u64.pow(attempt);
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry exhausted with no error")))
}
