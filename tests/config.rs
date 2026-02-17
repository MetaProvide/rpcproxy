use clap::Parser;
use rpcproxy::config::{validate_token, Config};

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
    assert!(!config.health);
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

#[test]
fn token_validation_accepts_valid_tokens() {
    assert!(validate_token("simple").is_ok());
    assert!(validate_token("with-dash").is_ok());
    assert!(validate_token("with_underscore").is_ok());
    assert!(validate_token("with.dot").is_ok());
    assert!(validate_token("with~tilde").is_ok());
    assert!(validate_token("ABC123xyz-_.~test").is_ok());
    assert!(validate_token("1234567890").is_ok());
}

#[test]
fn token_validation_rejects_empty() {
    assert!(validate_token("").is_err());
}

#[test]
fn token_validation_rejects_invalid_chars() {
    assert!(validate_token("has space").is_err());
    assert!(validate_token("has/slash").is_err());
    assert!(validate_token("has?question").is_err());
    assert!(validate_token("has#hash").is_err());
    assert!(validate_token("has!bang").is_err());
    assert!(validate_token("has@at").is_err());
    assert!(validate_token("has%percent").is_err());
}

#[test]
fn health_flag_parsed() {
    let config = Config::parse_from(["rpcproxy", "--health"]);
    assert!(config.health);
}

#[test]
fn health_flag_with_custom_port() {
    let config = Config::parse_from(["rpcproxy", "--health", "--port", "7777"]);
    assert!(config.health);
    assert_eq!(config.port, 7777);
}
