# rpcproxy

High-performance JSON-RPC reverse proxy written in Rust. Designed as a drop-in replacement for [etherproxy](https://github.com/MetaProvide/etherproxy) with smart caching, priority-based failover, and active health checking.

## Features

- **Priority-based failover** — routes requests through ordered upstream RPCs, automatically skipping unhealthy backends
- **Reactive health checking** — backends that fail during normal traffic trigger an immediate health probe instead of waiting for the next periodic interval
- **Smart caching** — method-aware TTLs (immutable data cached for 1 hour, chain-tip data for configurable TTL)
- **Request coalescing** — deduplicates identical in-flight requests to reduce upstream load
- **Stale block detection** — backends reporting blocks far behind the best known block are marked degraded
- **URL path token auth** — optional token as URL path so clients that can only provide a URL (e.g. Bee) can authenticate
- **Bearer token auth** — standard `Authorization: Bearer <token>` header support
- **Batch JSON-RPC** — full support for batched requests
- **Verbose mode** — toggle between detailed debug logs and quiet production logging
- **Docker ready** — multi-arch image with built-in `HEALTHCHECK`

## Quick Start

### Download a binary

Pre-built binaries for Linux, macOS, and Windows are attached to each [GitHub Release](https://github.com/MetaProvide/rpcproxy/releases).

```bash
# Example: Linux amd64
curl -LO https://github.com/MetaProvide/rpcproxy/releases/latest/download/rpcproxy-linux-amd64
chmod +x rpcproxy-linux-amd64
mv rpcproxy-linux-amd64 rpcproxy
./rpcproxy --targets https://rpc.gnosis.gateway.fm --verbose
```

### Docker

```bash
docker pull ghcr.io/metaprovide/rpcproxy:latest

docker run -p 9000:9000 ghcr.io/metaprovide/rpcproxy:latest \
  --targets https://rpc.gnosis.gateway.fm,https://rpc.ankr.com/gnosis
```

### From source

```bash
cargo build --release
./target/release/rpcproxy --targets https://rpc.gnosis.gateway.fm --verbose
```

## Configuration

All options can be set via CLI flags or environment variables.

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--port` | `RPCPROXY_PORT` | `9000` | Port to listen on |
| `--targets` | `RPCPROXY_TARGETS` | `http://localhost:8545` | Comma-separated upstream RPC URLs (priority order) |
| `--cache-ttl` | `RPCPROXY_CACHE_TTL` | `2000` | Default cache TTL in milliseconds |
| `--health-interval` | `RPCPROXY_HEALTH_INTERVAL` | `1800` | Health check interval in seconds |
| `--request-timeout` | `RPCPROXY_REQUEST_TIMEOUT` | `10` | Upstream request timeout in seconds |
| `--cache-max-size` | `RPCPROXY_CACHE_MAX_SIZE` | `10000` | Maximum number of cached entries |
| `--token` | `RPCPROXY_TOKEN` | _(none)_ | Token for URL-path authentication (see below) |
| `-v, --verbose` | `RPCPROXY_VERBOSE` | `false` | Enable detailed debug logging |

### Example with Docker Compose

```yaml
services:
  rpcproxy:
    image: ghcr.io/metaprovide/rpcproxy:latest
    environment:
      RPCPROXY_TARGETS: https://rpc.gnosis.gateway.fm,https://rpc.ankr.com/gnosis
      RPCPROXY_CACHE_TTL: 2000
      RPCPROXY_TOKEN: my-secret-token
    ports:
      - "9000:9000"
```

### Example with Bee (dockerbee)

Since Bee can only provide a URL for its RPC endpoint (no custom headers), the token
becomes part of the URL path:

```yaml
services:
  bee:
    environment:
      BEE_BLOCKCHAIN_RPC_ENDPOINT: http://rpcproxy:9000/my-secret-token

  rpcproxy:
    image: ghcr.io/metaprovide/rpcproxy:latest
    environment:
      RPCPROXY_TARGETS: https://rpc.gnosis.gateway.fm
      RPCPROXY_TOKEN: my-secret-token
```

## Endpoints

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `POST /` | POST | Bearer | JSON-RPC proxy when `--token` is set |
| `POST /<token>` | POST | Path | JSON-RPC proxy when `--token` is set |
| `/health` | GET | No | Returns `200 ok` if ≥1 backend has real block data, else `503` |
| `/readiness` | GET | Bearer | JSON response with backend details and overall status |
| `/status` | GET | Bearer | Detailed JSON: all backends, states, request counts, cache stats |

### Authentication

When `--token` is set, RPC requests can authenticate in two ways:

**1. URL Path Token**
```bash
curl -X POST http://localhost:9000/my-secret-token -d '{...}'
```

**2. Bearer Header**
```bash
curl -X POST http://localhost:9000 -H "Authorization: Bearer my-secret-token" -d '{...}'
```

The `/health` endpoint is **not** protected (for Docker HEALTHCHECK).
When a token is set, `/readiness` and `/status` require an `Authorization: Bearer <token>` header.

### Status response example

```json
{
  "healthy_backends": 2,
  "total_backends": 3,
  "cache_entries": 42,
  "backends": [
    {
      "url": "https://rpc.gnosis.gateway.fm",
      "priority": 0,
      "state": "Healthy",
      "latency_ms": 120.5,
      "latest_block": 44662374,
      "total_requests": 1500,
      "total_errors": 3,
      "uptime_secs": 86400
    }
  ]
}
```

## How It Works

### Failover

Backends are tried in the order they are listed in `--targets`. If a backend returns an error, the next one is tried. After all backends have been attempted, the first backend gets one last-resort retry. A backend is marked **Down** after 3 consecutive errors and is skipped entirely until the health checker restores it.

### Reactive Health Checking

Health checking runs in two modes simultaneously:

1. **Periodic** — every `--health-interval` seconds, all backends are probed with `eth_blockNumber`
2. **Reactive** — when a backend transitions to **Down** during normal request forwarding, the health checker is immediately notified and re-probes all backends without waiting for the next interval

This ensures that a recovered backend is discovered within seconds rather than waiting up to 30 minutes (the default interval).

### Backend States

| State | Meaning |
|-------|---------|
| **Healthy** | Responding normally |
| **Degraded** | Responding, but latest block is >10 blocks behind the best backend |
| **Down** | 3+ consecutive errors; skipped for traffic until health check restores it |

### Caching Strategy

| Category | TTL | Examples |
|----------|-----|---------|
| **Immutable** | 1 hour | `eth_getTransactionReceipt`, `eth_getBlockByHash`, `eth_chainId`, `net_version` |
| **Immutable (conditional)** | 1 hour | `eth_getBlockByNumber` with hex block, `eth_getLogs` with `blockHash` |
| **Chain-tip** | `--cache-ttl` | `eth_blockNumber`, `eth_gasPrice`, `eth_getBalance` |
| **Never cached** | — | `eth_sendRawTransaction`, `personal_sign`, `debug_*` |

Identical in-flight requests are coalesced — only one upstream call is made, and all waiting clients receive the same response.

## Logging

- **Default**: startup info, backend state changes, errors, and warnings only
- **Verbose (`-v`)**: per-request debug logs — cache hits/misses, upstream selection, latencies, health check probes

`RUST_LOG` env var takes precedence if set.

## Development

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test
```

Tests cover configuration, JSON-RPC parsing, cache policy, backend state machine, upstream failover, HTTP handler auth/routing, and reactive health checking. All tests use [wiremock](https://docs.rs/wiremock) for deterministic mock servers.

### Lint

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

### Build Docker Image

```bash
docker build -t rpcproxy:latest .
```

The Dockerfile uses a multi-stage build with [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) for dependency layer caching.

## CI/CD

| Workflow | Trigger | What it does |
|----------|---------|-------------|
| **CI** | Push / PR to `main` | `cargo check`, `cargo fmt`, `cargo clippy`, `cargo test` |
| **Release Binaries** | GitHub Release published | Builds binaries for Linux (amd64/arm64), macOS (amd64/arm64), Windows (amd64) and attaches them to the release |
| **Docker Image** | Tag push / Release | Builds and pushes multi-arch Docker image to `ghcr.io/metaprovide/rpcproxy` |

## License

MIT
