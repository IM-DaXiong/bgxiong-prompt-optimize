use std::future::Future;
use std::time::Duration;

use crate::openai::outbound_http::error::OutboundError;
use crate::openai::outbound_http::ports::CancelSignal;

include!("app_contracts_network_resilience.rs");

#[cfg(test)]
pub fn window_minutes() -> u64 {
    NETWORK_INTERRUPTION_WINDOW_MINUTES
}

pub fn window_duration() -> Duration {
    Duration::from_secs(NETWORK_INTERRUPTION_WINDOW_MINUTES.saturating_mul(60))
}

pub fn window_ms() -> u64 {
    NETWORK_INTERRUPTION_WINDOW_MINUTES.saturating_mul(60_000)
}

pub fn backoff_ms(attempt: u32) -> u64 {
    let exp = 2u64.saturating_pow(attempt.min(4));
    NETWORK_BACKOFF_BASE_MS
        .saturating_mul(exp)
        .min(NETWORK_BACKOFF_CAP_MS)
}

pub async fn sleep_or_cancel(
    sleep_ms: u64,
    cancel: Option<&dyn CancelSignal>,
) -> Result<(), OutboundError> {
    let sleep = tokio::time::sleep(Duration::from_millis(sleep_ms));
    tokio::pin!(sleep);
    if let Some(c) = cancel {
        loop {
            tokio::select! {
                _ = &mut sleep => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if c.is_cancelled().await {
                        return Err(OutboundError::Cancelled);
                    }
                }
            }
        }
    } else {
        sleep.await;
        Ok(())
    }
}

/// Race an in-flight future against `CancelSignal` (INV-TTS-SYNTH-CANCEL-01).
/// Dropping `fut` on cancel aborts reqwest send / body read.
pub async fn await_or_cancel<T>(
    fut: impl Future<Output = T>,
    cancel: Option<&dyn CancelSignal>,
) -> Result<T, OutboundError> {
    tokio::pin!(fut);
    if let Some(c) = cancel {
        loop {
            tokio::select! {
                r = &mut fut => return Ok(r),
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if c.is_cancelled().await {
                        return Err(OutboundError::Cancelled);
                    }
                }
            }
        }
    } else {
        Ok(fut.await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_is_contract_30() {
        assert_eq!(NETWORK_INTERRUPTION_WINDOW_MINUTES, 30);
        assert_eq!(NETWORK_BACKOFF_BASE_MS, 2000);
        assert_eq!(NETWORK_BACKOFF_CAP_MS, 30000);
        assert_eq!(backoff_ms(0), 2000);
        assert_eq!(backoff_ms(99), 30000);
    }
}
