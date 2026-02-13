use clap::Parser;
use rpcproxy::config::Config;

#[test]
fn defaults() {
    let config = Config::parse_from(["rpcproxy"]);
    assert_eq!(config.port, 9000);
    assert_eq!(config.targets, vec!["http://localhost:8545"]);
    assert_eq!(config.cache_ttl, 2000);
    assert_eq!(config.health_interval, 1800);
    assert_eq!(config.request_timeout, 10);
    assert_eq!(config.cache_max_size, 10000);
    assert!(config.token.is_none());
}

#[test]
fn cli_overrides() {
    let config = Config::parse_from([
        "rpcproxy",
        "--port",
        "8080",
        "--targets",
        "http://a.com,http://b.com",
        "--cache-ttl",
        "5000",
        "--health-interval",
        "30",
        "--request-timeout",
        "20",
        "--cache-max-size",
        "50000",
        "--token",
        "secret123",
    ]);
    assert_eq!(config.port, 8080);
    assert_eq!(config.targets, vec!["http://a.com", "http://b.com"]);
    assert_eq!(config.cache_ttl, 5000);
    assert_eq!(config.health_interval, 30);
    assert_eq!(config.request_timeout, 20);
    assert_eq!(config.cache_max_size, 50000);
    assert_eq!(config.token, Some("secret123".to_string()));
}
