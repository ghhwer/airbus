//! Listener registration and shared retry/exhaustion rules.
use crate::runtime::UuidV7;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ListenerRegistration {
    pub id: UuidV7,
    pub host: String,
    pub port: u16,
    pub failure_count: u64,
    pub unreachable_since: Option<Instant>,
    pub max_retries: u64,
    pub exhaustion_timeout: Duration,
}

impl ListenerRegistration {
    pub fn new(
        id: UuidV7,
        host: String,
        port: u16,
        max_retries: u64,
        exhaustion_timeout: Duration,
    ) -> Self {
        Self {
            id,
            host,
            port,
            failure_count: 0,
            unreachable_since: None,
            max_retries,
            exhaustion_timeout,
        }
    }

    /// Record an acknowledgment or failure; true means the registration is exhausted.
    pub(super) fn record_delivery(&mut self, delivered: bool, now: Instant) -> bool {
        if delivered {
            self.failure_count = 0;
            self.unreachable_since = None;
            return false;
        }
        self.failure_count += 1;
        let since = *self.unreachable_since.get_or_insert(now);
        self.failure_count >= self.max_retries
            || now.duration_since(since) >= self.exhaustion_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration() -> ListenerRegistration {
        ListenerRegistration::new(
            UuidV7::generate(),
            "127.0.0.1".into(),
            12345,
            2,
            Duration::from_secs(10),
        )
    }

    #[test]
    fn acknowledgment_resets_the_consecutive_failure_budget() {
        let mut listener = registration();
        let now = Instant::now();
        assert!(!listener.record_delivery(false, now));
        assert!(!listener.record_delivery(true, now + Duration::from_secs(1)));
        assert_eq!(listener.failure_count, 0);
        assert_eq!(listener.unreachable_since, None);
        assert!(!listener.record_delivery(false, now + Duration::from_secs(11)));
        assert!(listener.record_delivery(false, now + Duration::from_secs(12)));
    }

    #[test]
    fn timeout_exhausts_a_listener_before_the_retry_limit() {
        let mut listener = registration();
        listener.max_retries = 100;
        let now = Instant::now();
        assert!(!listener.record_delivery(false, now));
        assert!(!listener.record_delivery(false, now + Duration::from_secs(9)));
        assert!(listener.record_delivery(false, now + Duration::from_secs(10)));
    }
}
