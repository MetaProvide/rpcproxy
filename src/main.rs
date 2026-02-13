use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use tracing::info;

use rpcproxy::cache::RpcCache;
use rpcproxy::config::Config;
use rpcproxy::handler;
use rpcproxy::handler::AppState;
use rpcproxy::health;
use rpcproxy::upstream::UpstreamManager;

#[tokio::main]
async fn main() {
    let config = Config::parse();

    let log_level = if config.verbose {
        "debug,hyper=info,reqwest=info"
    } else {
        "warn,rpcproxy=info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    let token = config.token.clone().filter(|t| !t.is_empty());

    info!(
        port = %config.port,
        targets = ?config.targets,
        cache_ttl = %config.cache_ttl,
        health_interval = %config.health_interval,
        auth = token.is_some(),
        verbose = config.verbose,
        "starting rpcproxy"
    );

    if let Some(ref t) = token {
        info!(path = %format!("/{t}"), "token auth enabled via URL path");
    }

    let upstream = Arc::new(UpstreamManager::new(
        config.targets.clone(),
        Duration::from_secs(config.request_timeout),
    ));

    let cache = RpcCache::new(config.cache_max_size, config.cache_ttl);

    let state = AppState {
        upstream: upstream.clone(),
        cache,
        token,
    };

    // Spawn health checker
    tokio::spawn(health::start_health_checker(
        upstream.clone(),
        config.health_interval,
    ));

    let app = Router::new()
        .route("/health", get(handler::status::health_handler))
        .route("/readiness", get(handler::status::readiness_handler))
        .route("/status", get(handler::status::status_handler))
        .route("/{token}", post(handler::rpc::token_rpc_handler))
        .fallback(post(handler::rpc::open_rpc_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    info!(addr = %addr, "rpcproxy listening");
    axum::serve(listener, app).await.expect("server error");
}
