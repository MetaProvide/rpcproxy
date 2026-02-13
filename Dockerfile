FROM rust:1.93-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /usr/src/rpcproxy

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /usr/src/rpcproxy/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
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
