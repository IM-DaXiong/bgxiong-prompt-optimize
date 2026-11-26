
use crate::openai::outbound_http::error::format_reqwest_source_chain;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptClass {
    Connect,
    InFlightCut,
}

/// `send()` failed with no `Response`. `is_body()` during this phase is InFlightCut (H-26).
pub fn classify_send_error(e: &reqwest::Error) -> InterruptClass {
    if e.is_connect() {
        InterruptClass::Connect
    } else {
        InterruptClass::InFlightCut
    }
}

/// Certificate / bad URL / redirect loop: will not recover by waiting (H-30).
pub fn is_unrecoverable(e: &reqwest::Error) -> bool {
    if e.is_builder() || e.is_redirect() {
        return true;
    }
    let mut cur: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = cur {
        let dbg = format!("{err:?}");
        if dbg.contains("InvalidCertificate")
            || dbg.contains("InvalidDnsName")
            || dbg.contains("NotValidForName")
            || dbg.contains("WebPki")
            || dbg.contains("CertificateNotValid")
            || dbg.contains("UnknownIssuer")
        {
            return true;
        }
        cur = err.source();
    }
    let chain = format_reqwest_source_chain(e);
    let lower = chain.to_ascii_lowercase();
    lower.contains("certificate verify failed")
        || lower.contains("unknown ca")
        || lower.contains("cert verify")
}
