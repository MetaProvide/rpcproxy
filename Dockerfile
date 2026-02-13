FROM rust:1.86-bookworm AS builder

WORKDIR /usr/src/rpcproxy
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/rpcproxy/target/release/rpcproxy /usr/local/bin/rpcproxy

USER nobody
EXPOSE 9000

HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -sf http://localhost:9000/health || exit 1

ENTRYPOINT ["rpcproxy"]
