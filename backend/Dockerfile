# syntax=docker/dockerfile:1

# ---------- Build stage ----------
FROM rust@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS builder
WORKDIR /build

# curl is used by utoipa-swagger-ui to download its UI assets at build time.
# The `openssl` crate (pulled in transitively by rust-s3) is built with its
# `vendored` feature, so perl + make compile a static OpenSSL into the fully
# static musl binary (Alpine's openssl-dev only ships shared libraries).
RUN apk add --no-cache curl make musl-dev perl pkgconf

# Cache dependencies by compiling a minimal stub first, so the real build
# only recompiles the crate itself.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
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
    && cargo build --release --locked

# ---------- Runtime stage ----------
FROM alpine@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6
# `curl` is required by the HEALTHCHECK below. The dedicated `app` account
# (uid/gid 1000) matches the Docker Compose bind mounts: `./tiles` is host-owned
# by the developer's uid 1000, so the basemap can be written without running the
# container as root.
RUN apk add --no-cache ca-certificates curl \
    && addgroup -g 1000 app \
    && adduser -u 1000 -G app -D -s /sbin/nologin app

WORKDIR /app
COPY --from=builder /build/target/release/bike_counter /usr/local/bin/bike_counter
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# The app reads the bind-mounted config.toml and listens on 8080 (>1024); it
# only writes to the /data bind mount, so it never needs root.
USER app

EXPOSE 8080
ENTRYPOINT ["/entrypoint.sh"]

# The container is healthy only while the application reports itself ready,
# i.e. while PostgreSQL answers the readiness probe.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl --fail --silent http://localhost:8080/health/ready || exit 1
