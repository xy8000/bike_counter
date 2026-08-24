#!/usr/bin/env bash
# CI-style formatting and lint gate.
#
# Fails (non-zero exit) when:
#   * `cargo fmt --check` reports formatting drift, or
#   * `cargo clippy --all-targets -- -D warnings` reports any warning.
#
# Run locally before pushing; wire it into CI to keep the tree clean.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# The Rust crate lives in backend/; the script operates on that directory.
cd "${PROJECT_ROOT}/backend"

echo "--- cargo fmt --check"
cargo fmt --check

echo "--- cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings

echo "fmt-test: OK"
