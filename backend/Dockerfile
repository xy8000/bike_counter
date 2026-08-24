# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM rust:1-alpine AS builder
WORKDIR /build

# utoipa-swagger-ui downloads its UI assets at build time via curl.
RUN apk add --no-cache curl

# Cache dependencies by compiling a minimal stub first, so the real build
# only recompiles the crate itself.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Build the real application.
# Docker COPY preserves the host files' original mtimes, which may be older than
# the stub build's Cargo fingerprint. Bump the mtimes so Cargo recompiles the
# crate (while still reusing the already-cached dependencies).
COPY src ./src
COPY migrations ./migrations
RUN find src migrations -type f -exec touch {} + \
    && cargo build --release

# ---------- Runtime stage ----------
FROM alpine:3.24
# `curl` is required by the HEALTHCHECK below.
RUN apk add --no-cache ca-certificates curl

WORKDIR /app
COPY --from=builder /build/target/release/bike_counter /usr/local/bin/bike_counter
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 8080
ENTRYPOINT ["/entrypoint.sh"]

# The container is healthy only while the application reports itself ready,
# i.e. while PostgreSQL answers the readiness probe.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl --fail --silent http://localhost:8080/health/ready || exit 1
