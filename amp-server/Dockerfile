# Fly.io Dockerfile for amp-server — the matchmaker (public HTTP + WS).
# Deploy from the REPO ROOT:  fly deploy -c amp-server/fly.toml
# Build context is the repo root (workspace Cargo.toml/lock + migrations).

FROM rust:1.97 AS builder
WORKDIR /amp
COPY Cargo.toml Cargo.lock ./
COPY amp-match-core/ ./amp-match-core/
COPY amp-server/ ./amp-server/
RUN cargo build --release -p amp-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /amp/target/release/amp-server /usr/local/bin/amp-server
# sqlx::migrate! embeds ../migrations at build time; no runtime copy needed.
EXPOSE 8080
CMD ["amp-server"]
