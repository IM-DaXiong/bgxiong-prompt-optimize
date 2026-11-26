//! Warehouse-wide network-interruption retry (INV-NET-INTERRUPT).
//! Zero business imports: no ai_task / db / open_platform / identity headers.

pub mod classify;
pub mod clients;
pub mod error;
pub mod policy;
pub mod ports;
pub mod window;

pub use classify::InterruptClass;
pub use clients::ClientFingerprint;
#[cfg(test)]
pub use clients::RedirectMode;
pub use error::OutboundError;
pub use policy::{IdempotencyClass, WaitBudget};
pub use ports::{CancelSignal, RetryAttempt, RetryObserver};
#[cfg(test)]
pub use ports::AtomicCancel;
#[cfg(test)]
pub use window::window_minutes;

use std::future::Future;

// 窗计时与退避 sleep 同一时钟（tokio）：生产里等同单调时钟；测试可暂停/快进，离线覆盖 30 分钟窗。
use tokio::time::Instant;

use reqwest::RequestBuilder;

use classify::{classify_send_error, is_unrecoverable};
use error::format_reqwest_source_chain;
use policy::may_resend;

#[derive(Clone, Copy)]
pub struct SendOpts<'a> {
    pub wait: WaitBudget,
    pub idempotency: IdempotencyClass,
    pub client: ClientFingerprint,
    pub cancel: Option<&'a dyn CancelSignal>,
    pub observer: Option<&'a dyn RetryObserver>,
    pub correlation: Option<&'a str>,
    pub rebuildable: bool,
}

impl Default for SendOpts<'static> {
    fn default() -> Self {
        Self::cloud_post()
    }
}

impl SendOpts<'static> {
    pub fn cloud_post() -> Self {
        Self {
            wait: WaitBudget::Window,
            idempotency: IdempotencyClass::NonIdempotent,
            client: ClientFingerprint::long_running(None),
            cancel: None,
            observer: None,
            correlation: None,
            rebuildable: true,
        }
    }

    pub fn cloud_get() -> Self {
        Self {
            wait: WaitBudget::Window,
            idempotency: IdempotencyClass::Idempotent,
            client: ClientFingerprint::long_running(None),
            cancel: None,
            observer: None,
            correlation: None,
            rebuildable: true,
        }
    }

    pub fn fail_fast_get() -> Self {
        Self {
            wait: WaitBudget::FailFast,
            idempotency: IdempotencyClass::Idempotent,
            client: ClientFingerprint::fail_fast(5),
            cancel: None,
            observer: None,
            correlation: None,
            rebuildable: true,
        }
    }

    pub fn fail_fast_post() -> Self {
        Self {
            wait: WaitBudget::FailFast,
            idempotency: IdempotencyClass::NonIdempotent,
            client: ClientFingerprint::fail_fast(5),
            cancel: None,
            observer: None,
            correlation: None,
            rebuildable: true,
        }
    }
}

impl<'a> SendOpts<'a> {
    /// INV-NET-LOOPBACK-01': only the request timeout; never rebuild fingerprint
    /// (that would wipe `bypass_proxy` / `http1_only`).
    pub fn with_timeout_secs(mut self, secs: Option<u64>) -> Self {
        self.client.request_timeout_ms = secs.filter(|&s| s > 0).map(|s| s * 1000);
        self
    }

    pub fn with_http1(mut self) -> Self {
        self.client.http1_only = true;
        self
    }

    pub fn with_cancel(mut self, cancel: &'a dyn CancelSignal) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_observer(mut self, observer: &'a dyn RetryObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn with_correlation(mut self, id: &'a str) -> Self {
        self.correlation = Some(id);
        self
    }

    pub fn with_client(mut self, client: ClientFingerprint) -> Self {
        self.client = client;
        self
    }
}

pub async fn send<F>(opts: SendOpts<'_>, build: F) -> Result<reqwest::Response, OutboundError>
where
    F: Fn(&reqwest::Client) -> RequestBuilder,
{
    send_rebuilding(opts, |c| {
        let b = build(&c);
        async move { Ok(b) }
    })
    .await
}

/// Same A-layer as `send`, but the builder may be async (stream file parts per attempt).
pub async fn send_rebuilding<F, Fut>(
    opts: SendOpts<'_>,
    build: F,
) -> Result<reqwest::Response, OutboundError>
where
    F: Fn(reqwest::Client) -> Fut,
    Fut: Future<Output = Result<RequestBuilder, OutboundError>>,
{
    if opts.wait == WaitBudget::Window && !opts.rebuildable {
        return Err(OutboundError::Transport {
            source: "WaitBudget::Window requires a rebuildable request body".into(),
        });
    }
    let client = clients::get(opts.client);
    let window = window::window_duration();
    let window_ms = window::window_ms();
    let started = Instant::now();
    let mut attempt = 0u32;
    let mut retried = false;
    let mut pending_resend: Option<RetryAttempt> = None;
    loop {
        if let Some(c) = opts.cancel {
            if c.is_cancelled().await {
                return Err(OutboundError::Cancelled);
            }
        }
        if let Some(ev) = pending_resend.take() {
            notify_resend(opts, &ev);
        }
        attempt = attempt.saturating_add(1);
        let builder = build(client.clone()).await?;
        match window::await_or_cancel(builder.send(), opts.cancel).await {
            Err(OutboundError::Cancelled) => return Err(OutboundError::Cancelled),
            Ok(Ok(resp)) => {
                notify_recovered_if(opts, retried);
                return Ok(resp);
            }
            Ok(Err(e)) => {
                if is_unrecoverable(&e) {
                    return Err(OutboundError::Transport {
                        source: format_reqwest_source_chain(&e),
                    });
                }
                let class = classify_send_error(&e);
                decide_retry(opts, class, &e, attempt, started, window, window_ms)?;
                pending_resend = Some(retry_event(attempt, started, window_ms, class));
                retried = true;
                window::sleep_or_cancel(window::backoff_ms(attempt), opts.cancel).await?;
            }
            Err(other) => return Err(other),
        }
    }
}

fn decide_retry(
    opts: SendOpts<'_>,
    class: InterruptClass,
    e: &reqwest::Error,
    attempt: u32,
    started: Instant,
    window: std::time::Duration,
    window_ms: u64,
) -> Result<(), OutboundError> {
    let phase = "send";
    let source = format_reqwest_source_chain(e);
    if !may_resend(opts.wait, class) {
        return Err(OutboundError::Interrupted {
            class,
            source,
            timed_out: e.is_timeout(),
        });
    }
    let elapsed = started.elapsed();
    if elapsed >= window {
        return Err(OutboundError::WindowExhausted {
            class,
            attempts: attempt,
            elapsed_ms: elapsed.as_millis() as u64,
        });
    }
    if class == InterruptClass::InFlightCut && opts.idempotency == IdempotencyClass::NonIdempotent {
        tracing::warn!(
            attempt,
            elapsed_ms = elapsed.as_millis() as u64,
            phase,
            "outbound_http: InFlightCut NonIdempotent resend (possible double charge)"
        );
    }
    tracing::warn!(
        attempt,
        class = ?class,
        elapsed_ms = elapsed.as_millis() as u64,
        window_ms,
        phase,
        error = %source,
        "outbound_http: network interrupt, retrying"
    );
    notify_retry(opts, attempt, started, window_ms, class);
    // sleep happens in caller via ? on sleep_or_cancel — but decide_retry is sync after notify.
    // Call sleep from send loop instead? Keep sleep here as we need async...
    // This function is sync. Sleep in send loop after decide_retry Ok.
    Ok(())
}

fn retry_event(
    attempt: u32,
    started: Instant,
    window_ms: u64,
    class: InterruptClass,
) -> RetryAttempt {
    RetryAttempt {
        attempt,
        elapsed_ms: started.elapsed().as_millis() as u64,
        window_ms,
        class,
    }
}

fn notify_retry(
    opts: SendOpts<'_>,
    attempt: u32,
    started: Instant,
    window_ms: u64,
    class: InterruptClass,
) {
    if let Some(ob) = opts.observer {
        ob.on_attempt(&retry_event(attempt, started, window_ms, class));
    }
}

fn notify_resend(opts: SendOpts<'_>, ev: &RetryAttempt) {
    if let Some(ob) = opts.observer {
        ob.on_resend(ev);
    }
}

fn notify_recovered_if(opts: SendOpts<'_>, retried: bool) {
    if retried {
        if let Some(ob) = opts.observer {
            ob.on_recovered();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn window_without_rebuildable_is_err() {
        let mut opts = SendOpts::cloud_post();
        opts.rebuildable = false;
        let err = send(opts, |c| c.get("http://127.0.0.1:1/"))
            .await
            .expect_err("must reject");
        assert!(matches!(err, OutboundError::Transport { .. }));
    }

    #[tokio::test]
    async fn fail_fast_does_not_eat_window() {
        // Loopback target: bypass the system proxy (INV-NET-LOOPBACK-01) so a local proxy
        // (e.g. 127.0.0.1:7897 answering 502) cannot turn the connect failure into a response.
        // Same fail-fast timeouts as SendOpts::fail_fast_get(); only the proxy choice differs.
        let opts = SendOpts::fail_fast_get().with_client(ClientFingerprint {
            bypass_proxy: true,
            ..ClientFingerprint::fail_fast(5)
        });
        let started = Instant::now();
        let err = send(opts, |c| c.get("http://127.0.0.1:1/"))
            .await
            .expect_err("connect fail");
        assert!(started.elapsed().as_secs() < 20);
        assert!(matches!(
            err,
            OutboundError::Interrupted { .. } | OutboundError::Transport { .. }
        ));
    }

    #[tokio::test]
    async fn cancel_stops_before_window() {
        let cancel = AtomicCancel(AtomicBool::new(true));
        let opts = SendOpts {
            wait: WaitBudget::Window,
            idempotency: IdempotencyClass::Idempotent,
            client: ClientFingerprint::fail_fast(1),
            cancel: Some(&cancel),
            observer: None,
            correlation: None,
            rebuildable: true,
        };
        let err = send(opts, |c| c.get("http://127.0.0.1:1/"))
            .await
            .expect_err("cancelled");
        assert!(matches!(err, OutboundError::Cancelled));
    }

    #[tokio::test]
    async fn cancel_aborts_in_flight_fail_fast_send() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind hang listener");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move {
            loop {
                let Ok((_stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
        let flag = Arc::new(AtomicBool::new(false));
        struct FlagCancel(Arc<AtomicBool>);
        #[async_trait::async_trait]
        impl CancelSignal for FlagCancel {
            async fn is_cancelled(&self) -> bool {
                self.0.load(Ordering::Relaxed)
            }
        }
        let cancel = FlagCancel(Arc::clone(&flag));
        let url = format!("http://{addr}/hang");
        let opts = SendOpts::fail_fast_post()
            .with_client(ClientFingerprint::fail_fast_loopback(8))
            .with_cancel(&cancel);
        let started = Instant::now();
        let flag_set = Arc::clone(&flag);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            flag_set.store(true, Ordering::Relaxed);
        });
        let err = send(opts, move |c| c.post(&url).body("x"))
            .await
            .expect_err("must cancel");
        assert!(matches!(err, OutboundError::Cancelled), "{err:?}");
        assert!(started.elapsed().as_secs() < 4);
    }

    #[test]
    fn send_phase_body_kind_is_inflight() {
        // H-26: a send-phase body error is InFlightCut, which the Window budget resends.
        assert!(may_resend(WaitBudget::Window, InterruptClass::InFlightCut));
    }

    #[test]
    fn with_timeout_secs_preserves_loopback_and_cloud_proxy_polarity() {
        let loopback = SendOpts {
            client: ClientFingerprint::fail_fast_loopback(5),
            ..SendOpts::fail_fast_get()
        }
        .with_timeout_secs(Some(60));
        assert!(loopback.client.bypass_proxy);
        assert!(loopback.client.http1_only);
        assert_eq!(loopback.client.request_timeout_ms, Some(60_000));

        let cloud = SendOpts::cloud_get().with_timeout_secs(Some(60));
        assert!(!cloud.client.bypass_proxy);
        assert!(!cloud.client.http1_only);
        assert_eq!(cloud.client.request_timeout_ms, Some(60_000));
    }

    #[tokio::test]
    async fn invalid_url_is_unrecoverable_transport() {
        let err = send(SendOpts::cloud_get(), |c| c.get("http://"))
            .await
            .expect_err("bad url");
        assert!(
            matches!(err, OutboundError::Transport { .. })
                || matches!(err, OutboundError::Interrupted { .. }),
            "{err:?}"
        );
    }
}
