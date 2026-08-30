#!/usr/bin/env bash
# CI-style dependency security audit.
#
# Fails (non-zero exit) when `cargo audit` reports any advisory (default
# policy: no ignore list). Run locally before pushing; wire it into CI to keep
# the tree free of known vulnerabilities.
#
# Requires: cargo-audit -> cargo install cargo-audit
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "ERROR: cargo-audit is not installed." >&2
    echo "" >&2
    echo "  Install it, then re-run:" >&2
    echo "    cargo install cargo-audit" >&2
    echo "" >&2
    exit 1
fi

# The Rust crate lives in backend/; the script operates on that directory.
cd "${PROJECT_ROOT}/backend"

echo "--- cargo audit"
cargo audit --quiet

echo "audit: OK"
