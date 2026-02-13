use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "rpcproxy", about = "High-performance JSON-RPC reverse proxy")]
pub struct Config {
    /// Port to listen on
    #[arg(long, env = "RPCPROXY_PORT", default_value = "9000")]
    pub port: u16,

    /// Comma-separated list of upstream RPC URLs (priority order)
    #[arg(long, env = "RPCPROXY_TARGETS", default_value = "http://localhost:8545", value_delimiter = ',')]
    pub targets: Vec<String>,

    /// Default cache TTL in milliseconds
    #[arg(long, env = "RPCPROXY_CACHE_TTL", default_value = "2000")]
    pub cache_ttl: u64,

    /// Health check interval in seconds
    #[arg(long, env = "RPCPROXY_HEALTH_INTERVAL", default_value = "1800")]
    pub health_interval: u64,

    /// Upstream request timeout in seconds
    #[arg(long, env = "RPCPROXY_REQUEST_TIMEOUT", default_value = "10")]
    pub request_timeout: u64,

    /// Maximum number of cached entries
    #[arg(long, env = "RPCPROXY_CACHE_MAX_SIZE", default_value = "10000")]
    pub cache_max_size: u64,

    /// Bearer token for authenticating RPC requests. If set, all RPC requests
    /// must be sent to `POST /<token>`. The `/readiness` and `/status` endpoints
    /// require `Authorization: Bearer <token>`. The `/health` endpoint is not protected.
    #[arg(long, env = "RPCPROXY_TOKEN")]
    pub token: Option<String>,

    /// Enable verbose logging. Shows detailed human-readable logs for every request,
    /// cache hit/miss, upstream selection, and health check.
    /// When off, only critical messages and status changes are logged.
    #[arg(short, long, env = "RPCPROXY_VERBOSE", default_value = "false")]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
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
    fn test_cli_overrides() {
        let config = Config::parse_from([
            "rpcproxy",
            "--port", "8080",
            "--targets", "http://a.com,http://b.com",
            "--cache-ttl", "5000",
            "--health-interval", "30",
            "--request-timeout", "20",
            "--cache-max-size", "50000",
            "--token", "secret123",
        ]);
        assert_eq!(config.port, 8080);
        assert_eq!(config.targets, vec!["http://a.com", "http://b.com"]);
        assert_eq!(config.cache_ttl, 5000);
        assert_eq!(config.health_interval, 30);
        assert_eq!(config.request_timeout, 20);
        assert_eq!(config.cache_max_size, 50000);
        assert_eq!(config.token, Some("secret123".to_string()));
    }
}
