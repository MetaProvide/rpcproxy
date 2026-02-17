FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /usr/src/rpcproxy

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /usr/src/rpcproxy/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:trixie-slim AS runtime
COPY --from=builder /usr/src/rpcproxy/target/release/rpcproxy /usr/local/bin/rpcproxy

USER nobody
EXPOSE 9000

HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD ["rpcproxy", "--health"]

ENTRYPOINT ["rpcproxy"]
