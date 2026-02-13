use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendState {
    Healthy,
    Degraded,
    Down,
}

#[derive(Debug)]
pub struct BackendStatus {
    pub url: String,
    pub state: BackendState,
    pub consecutive_errors: u32,
    pub consecutive_successes: u32,
    pub last_error_at: Option<Instant>,
    pub last_success_at: Option<Instant>,
    pub latest_block: Option<u64>,
    pub avg_latency_ms: f64,
    pub total_requests: u64,
    pub total_errors: u64,
    pub started_at: Instant,
}

impl BackendStatus {
    pub fn new(url: String) -> Self {
        Self {
            url,
            state: BackendState::Healthy,
            consecutive_errors: 0,
            consecutive_successes: 0,
            last_error_at: None,
            last_success_at: None,
            latest_block: None,
            avg_latency_ms: 0.0,
            total_requests: 0,
            total_errors: 0,
            started_at: Instant::now(),
        }
    }

    pub fn record_success(&mut self, latency_ms: f64) {
        self.total_requests += 1;
        self.consecutive_errors = 0;
        self.consecutive_successes += 1;
        self.last_success_at = Some(Instant::now());
        self.state = BackendState::Healthy;
        if self.avg_latency_ms == 0.0 {
            self.avg_latency_ms = latency_ms;
        } else {
            self.avg_latency_ms = self.avg_latency_ms * 0.8 + latency_ms * 0.2;
        }
    }

    pub fn record_error(&mut self) {
        self.total_requests += 1;
        self.total_errors += 1;
        self.consecutive_successes = 0;
        self.consecutive_errors += 1;
        self.last_error_at = Some(Instant::now());
        if self.consecutive_errors >= 3 {
            self.state = BackendState::Down;
        } else {
            self.state = BackendState::Degraded;
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendHealthInfo {
    pub url: String,
    pub priority: usize,
    pub state: String,
    pub latency_ms: f64,
    pub latest_block: Option<u64>,
    pub total_requests: u64,
    pub total_errors: u64,
    pub uptime_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_state_transitions() {
        let mut backend = BackendStatus::new("http://localhost:8545".to_string());
        assert_eq!(backend.state, BackendState::Healthy);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Degraded);
        assert_eq!(backend.consecutive_errors, 1);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Degraded);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Down);
        assert_eq!(backend.consecutive_errors, 3);

        backend.record_success(50.0);
        assert_eq!(backend.state, BackendState::Healthy);
        assert_eq!(backend.consecutive_errors, 0);
    }

    #[test]
    fn test_latency_tracking() {
        let mut backend = BackendStatus::new("http://localhost:8545".to_string());
        backend.record_success(100.0);
        assert_eq!(backend.avg_latency_ms, 100.0);

        backend.record_success(200.0);
        // 100 * 0.8 + 200 * 0.2 = 120
        assert!((backend.avg_latency_ms - 120.0).abs() < 0.01);
    }
}
