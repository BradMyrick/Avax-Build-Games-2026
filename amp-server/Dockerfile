# amp-server container image — build from the REPO ROOT so the workspace
# Cargo.toml/lock, sibling crates, and the root migrations/ dir are all in
# context (sqlx::migrate!("../migrations") embeds them at BUILD time).
#
#   docker build -f amp-server/Dockerfile -t amp-server .

FROM rust:1.97 AS builder
WORKDIR /amp
COPY Cargo.toml Cargo.lock ./
COPY amp-match-core/ ./amp-match-core/
COPY amp-server/ ./amp-server/
COPY migrations/ ./migrations/
RUN cargo build --release -p amp-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /amp/target/release/amp-server /usr/local/bin/amp-server
EXPOSE 8080
CMD ["amp-server"]
