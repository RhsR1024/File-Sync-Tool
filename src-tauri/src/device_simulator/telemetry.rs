use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct ProtocolFailureMetrics {
    parse_failures: AtomicU64,
    send_failures: AtomicU64,
    last_log_at_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolFailureSnapshot {
    pub parse_failures: u64,
    pub send_failures: u64,
}

impl ProtocolFailureMetrics {
    pub fn record_parse_failure(&self, now_ms: u64, log_interval_ms: u64) -> bool {
        self.parse_failures.fetch_add(1, Ordering::Relaxed);
        self.claim_log_slot(now_ms, log_interval_ms)
    }

    pub fn record_send_failure(&self, now_ms: u64, log_interval_ms: u64) -> bool {
        self.send_failures.fetch_add(1, Ordering::Relaxed);
        self.claim_log_slot(now_ms, log_interval_ms)
    }

    pub fn snapshot(&self) -> ProtocolFailureSnapshot {
        ProtocolFailureSnapshot {
            parse_failures: self.parse_failures.load(Ordering::Relaxed),
            send_failures: self.send_failures.load(Ordering::Relaxed),
        }
    }

    fn claim_log_slot(&self, now_ms: u64, log_interval_ms: u64) -> bool {
        if log_interval_ms == 0 {
            return true;
        }
        let mut observed = self.last_log_at_ms.load(Ordering::Relaxed);
        loop {
            if observed != 0 && now_ms.saturating_sub(observed) < log_interval_ms {
                return false;
            }
            match self.last_log_at_ms.compare_exchange_weak(
                observed,
                now_ms.max(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(current) => observed = current,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_failure_but_rate_limits_log_admission() {
        let metrics = ProtocolFailureMetrics::default();
        assert!(metrics.record_parse_failure(1_000, 10_000));
        assert!(!metrics.record_parse_failure(2_000, 10_000));
        assert!(!metrics.record_send_failure(10_999, 10_000));
        assert!(metrics.record_send_failure(11_000, 10_000));
        assert_eq!(
            metrics.snapshot(),
            ProtocolFailureSnapshot {
                parse_failures: 2,
                send_failures: 2,
            }
        );
    }
}
