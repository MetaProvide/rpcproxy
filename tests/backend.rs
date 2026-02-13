use rpcproxy::upstream::{BackendState, BackendStatus};

#[test]
fn state_transitions() {
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
fn latency_tracking() {
    let mut backend = BackendStatus::new("http://localhost:8545".to_string());
    backend.record_success(100.0);
    assert_eq!(backend.avg_latency_ms, 100.0);

    backend.record_success(200.0);
    // 100 * 0.8 + 200 * 0.2 = 120
    assert!((backend.avg_latency_ms - 120.0).abs() < 0.01);
}
