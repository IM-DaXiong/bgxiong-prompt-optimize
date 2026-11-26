use crate::openai::outbound_http::classify::InterruptClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyClass {
    Idempotent,
    NonIdempotent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitBudget {
    Window,
    FailFast,
}

pub fn may_resend(wait: WaitBudget, class: InterruptClass) -> bool {
    if wait == WaitBudget::FailFast {
        return false;
    }
    match class {
        InterruptClass::Connect | InterruptClass::InFlightCut => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_fast_never_resends() {
        assert!(!may_resend(WaitBudget::FailFast, InterruptClass::Connect));
    }

    #[test]
    fn window_inflight_resends() {
        assert!(may_resend(WaitBudget::Window, InterruptClass::InFlightCut));
    }
}
