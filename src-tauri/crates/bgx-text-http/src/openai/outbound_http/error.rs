use crate::openai::outbound_http::classify::InterruptClass;

#[derive(Debug, Clone)]
pub enum OutboundError {
    Interrupted {
        class: InterruptClass,
        source: String,
        timed_out: bool,
    },
    WindowExhausted {
        class: InterruptClass,
        attempts: u32,
        elapsed_ms: u64,
    },
    Cancelled,
    Transport {
        source: String,
    },
}

impl std::fmt::Display for OutboundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interrupted { class, source, .. } => {
                write!(f, "outbound interrupted ({class:?}): {source}")
            }
            Self::WindowExhausted {
                class,
                attempts,
                elapsed_ms,
            } => write!(
                f,
                "outbound window exhausted ({class:?}) attempts={attempts} elapsed_ms={elapsed_ms}"
            ),
            Self::Cancelled => write!(f, "outbound cancelled"),
            Self::Transport { source } => write!(f, "outbound transport: {source}"),
        }
    }
}

impl std::error::Error for OutboundError {}

pub fn format_reqwest_source_chain(e: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(next) = cur {
        s.push_str(": ");
        s.push_str(&next.to_string());
        cur = next.source();
    }
    s
}
