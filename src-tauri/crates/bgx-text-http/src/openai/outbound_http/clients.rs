use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use reqwest::redirect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedirectMode {
    Limited,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientFingerprint {
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: Option<u64>,
    pub http1_only: bool,
    pub redirect: RedirectMode,
    /// INV-NET-LOOPBACK-01: 127.0.0.1 / localhost must not use system-proxy.
    pub bypass_proxy: bool,
}

impl ClientFingerprint {
    /// Destination routing policy: literal loopback and localhost bypass the system proxy.
    pub fn for_destination_url(mut self, endpoint:&url::Url)->Self {
        if endpoint.host().is_some_and(|host| match host {
            url::Host::Domain(name)=>name.eq_ignore_ascii_case("localhost"),
            url::Host::Ipv4(address)=>address.is_loopback(),
            url::Host::Ipv6(address)=>address.is_loopback(),
        }) {self.bypass_proxy=true;}
        self
    }
    pub fn long_running(request_timeout_secs: Option<u64>) -> Self {
        Self {
            connect_timeout_ms: 30_000,
            request_timeout_ms: request_timeout_secs.filter(|&s| s > 0).map(|s| s * 1000),
            http1_only: false,
            redirect: RedirectMode::Limited,
            bypass_proxy: false,
        }
    }

    pub fn fail_fast(timeout_secs: u64) -> Self {
        Self {
            connect_timeout_ms: timeout_secs.saturating_mul(1000).max(500),
            request_timeout_ms: Some(timeout_secs.saturating_mul(1000).max(500)),
            http1_only: false,
            redirect: RedirectMode::Limited,
            bypass_proxy: false,
        }
    }

    pub fn fail_fast_no_redirect(timeout_secs: u64) -> Self {
        Self {
            connect_timeout_ms: timeout_secs.saturating_mul(1000).max(500),
            request_timeout_ms: Some(timeout_secs.saturating_mul(1000).max(500)),
            http1_only: false,
            redirect: RedirectMode::None,
            bypass_proxy: false,
        }
    }

    pub fn http1_long_running(request_timeout_secs: Option<u64>) -> Self {
        let mut f = Self::long_running(request_timeout_secs);
        f.http1_only = true;
        f
    }

    /// Loopback engine health / API: no system-proxy, HTTP/1, short connect vs request.
    pub fn fail_fast_loopback(timeout_secs: u64) -> Self {
        Self {
            connect_timeout_ms: 500,
            request_timeout_ms: Some(timeout_secs.saturating_mul(1000).max(500)),
            http1_only: true,
            redirect: RedirectMode::Limited,
            bypass_proxy: true,
        }
    }

    pub fn http1_loopback_long_running(request_timeout_secs: Option<u64>) -> Self {
        let mut f = Self::http1_long_running(request_timeout_secs);
        f.bypass_proxy = true;
        f
    }
}

static POOL: Lazy<Mutex<HashMap<ClientFingerprint, reqwest::Client>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn get(fp: ClientFingerprint) -> reqwest::Client {
    if let Ok(guard) = POOL.lock() {
        if let Some(c) = guard.get(&fp) {
            return c.clone();
        }
    }
    let client = build(fp).unwrap_or_else(|_| {
        let mut b = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(fp.connect_timeout_ms.max(500)));
        if fp.http1_only {
            b = b.http1_only();
        }
        if fp.bypass_proxy {
            b = b.no_proxy();
        }
        b.build().unwrap_or_else(|_| reqwest::Client::new())
    });
    if let Ok(mut guard) = POOL.lock() {
        guard.insert(fp, client.clone());
    }
    client
}

fn build(fp: ClientFingerprint) -> Result<reqwest::Client, reqwest::Error> {
    let mut b = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(fp.connect_timeout_ms.max(1)));
    if let Some(ms) = fp.request_timeout_ms {
        b = b.timeout(Duration::from_millis(ms.max(1)));
    }
    if fp.http1_only {
        b = b.http1_only();
    }
    if fp.bypass_proxy {
        b = b.no_proxy();
    }
    b = match fp.redirect {
        RedirectMode::Limited => b.redirect(redirect::Policy::limited(10)),
        RedirectMode::None => b.redirect(redirect::Policy::none()),
    };
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_fingerprint_bypasses_proxy_and_forces_http1() {
        let f = ClientFingerprint::fail_fast_loopback(3);
        assert!(f.bypass_proxy);
        assert!(f.http1_only);
        assert_eq!(f.connect_timeout_ms, 500);
        assert_eq!(f.request_timeout_ms, Some(3_000));
    }

    #[test]
    fn cloud_fail_fast_keeps_system_proxy() {
        let f = ClientFingerprint::fail_fast(3);
        assert!(!f.bypass_proxy);
        assert!(!f.http1_only);
    }

    #[test]
    fn loopback_long_running_bypasses_proxy() {
        let f = ClientFingerprint::http1_loopback_long_running(None);
        assert!(f.bypass_proxy);
        assert!(f.http1_only);
        let cloud = ClientFingerprint::http1_long_running(None);
        assert!(!cloud.bypass_proxy);
    }
}
