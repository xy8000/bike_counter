# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM rust:1-alpine AS builder
WORKDIR /build

# curl is used by utoipa-swagger-ui to download its UI assets at build time.
# The `openssl` crate (pulled in transitively by rust-s3) is built with its
# `vendored` feature, so perl + make compile a static OpenSSL into the fully
# static musl binary (Alpine's openssl-dev only ships shared libraries).
RUN apk add --no-cache curl perl make musl-dev pkgconf

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
# The built-in station image is embedded into the binary at compile time.
COPY assets ./assets
RUN find src migrations assets -type f -exec touch {} + \
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
