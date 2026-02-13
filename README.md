# rpcproxy

High-performance JSON-RPC reverse proxy written in Rust. Designed as a drop-in replacement for [etherproxy](https://github.com/MetaProvide/etherproxy) with smart caching, priority-based failover, and active health checking.

## Features

- **Priority-based failover** — routes requests through ordered upstream RPCs, skipping unhealthy backends
- **Smart caching** — method-aware TTLs (immutable data cached for 1 hour, chain-tip data for configurable TTL)
- **Request coalescing** — deduplicates identical in-flight requests to reduce upstream load
- **Active health checking** — periodic `eth_blockNumber` probes detect stale or down backends
- **URL path token auth** — optional token as URL path (like GetBlock) so clients that can only provide a URL (e.g. Bee) can authenticate
- **Batch JSON-RPC** — full support for batched requests
- **Verbose mode** — toggle between detailed debug logs and quiet production logging
- **Docker ready** — built-in `HEALTHCHECK` that validates at least one backend returns real block data

## Quick Start

### Docker (recommended)

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
| `--health-interval` | `RPCPROXY_HEALTH_INTERVAL` | `10` | Health check interval in seconds |
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
| `POST /` | POST | No | JSON-RPC proxy when no token is set |
| `POST /<token>` | POST | Yes | JSON-RPC proxy when `--token` is set |
| `/health` | GET | No | Returns `200 ok` if ≥1 backend has real block data, else `503` |
| `/readiness` | GET | Bearer | JSON response with backend details and overall status |
| `/status` | GET | Bearer | Detailed JSON: all backends, states, request counts, cache stats |

### Authentication

When `--token` is set, the token becomes a **URL path segment**. This design lets
clients that can only provide a URL (like Bee nodes) authenticate without custom headers,
similar to how [GetBlock](https://getblock.io) works:

```text
# No token set → open access
curl -X POST http://localhost:9000 -d '{...}'

# Token set → must use path
curl -X POST http://localhost:9000/my-secret-token -d '{...}'

# Without token path → 401 Unauthorized
curl -X POST http://localhost:9000 -d '{...}'
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

## Caching Strategy

| Category | TTL | Examples |
|----------|-----|---------|
| **Immutable** | 1 hour | `eth_getTransactionReceipt`, `eth_getBlockByHash`, `eth_chainId`, `net_version` |
| **Immutable (conditional)** | 1 hour | `eth_getBlockByNumber` with hex block, `eth_getLogs` with `blockHash` |
| **Chain-tip** | `--cache-ttl` | `eth_blockNumber`, `eth_gasPrice`, `eth_getBalance` |
| **Never cached** | — | `eth_sendRawTransaction`, `personal_sign`, `debug_*` |

## Verbose Mode

- **Off (default)**: logs startup info, backend state changes, errors, and warnings only
- **On (`-v`)**: adds per-request debug logs — cache hits/misses, upstream selection, latencies, health check probes

`RUST_LOG` env var always takes precedence if set.

## Building the Docker Image

```bash
docker build -t rpcproxy:latest .
```

The Dockerfile uses a multi-stage build with [cargo-chef](https://github.com/LukeMathWalker/cargo-chef) for dependency layer caching.
Subsequent builds after dependency changes are fast because only your source code is recompiled.

## License

MIT
