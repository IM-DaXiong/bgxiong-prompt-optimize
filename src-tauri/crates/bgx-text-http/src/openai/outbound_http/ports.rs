use async_trait::async_trait;

use crate::openai::outbound_http::classify::InterruptClass;

#[async_trait]
pub trait CancelSignal: Send + Sync {
    async fn is_cancelled(&self) -> bool;
}

pub trait RetryObserver: Send + Sync {
    fn on_attempt(&self, ev: &RetryAttempt);
    /// Called when this HTTP call succeeds after at least one `on_attempt`.
    /// Host must clear live `transportRetry` overlay (INV-NET-INTERRUPT-21).
    fn on_recovered(&self) {}
    /// Called right before a scheduled resend is actually sent (after backoff and the cancel
    /// check). A resend dropped by cancel during backoff never reaches here, so a per-attempt
    /// ledger written here matches real HTTP attempts.
    fn on_resend(&self, _ev: &RetryAttempt) {}
}

#[derive(Debug, Clone)]
pub struct RetryAttempt {
    pub attempt: u32,
    pub elapsed_ms: u64,
    pub window_ms: u64,
    pub class: InterruptClass,
}

#[cfg(test)]
pub struct AtomicCancel(pub std::sync::atomic::AtomicBool);

#[cfg(test)]
#[async_trait]
impl CancelSignal for AtomicCancel {
    async fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}
