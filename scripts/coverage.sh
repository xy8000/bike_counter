#!/usr/bin/env bash
# CI-style coverage gate using cargo-llvm-cov.
#
# Enforces two line-coverage thresholds from a single instrumented test run,
# both measured on PRODUCTION code only:
#   * overall (whole crate) -- COVERAGE_THRESHOLD (default 80)
#   * core (src/core/)      -- CORE_COVERAGE_THRESHOLD (default 95)
#
# "Production only" means lines belonging to `#[cfg(test)]` modules (skipped
# from the first "#[cfg(test)]" marker in each source file) and standalone test
# files (paths containing `/tests/`, or named `tests.rs`/`test.rs`) are excluded
# from the calculation. Test scaffolding must never inflate the number.
#
# The core is pure hexagonal logic and is expected to be fully unit-tested in
# isolation with in-memory mocks, hence its higher bar than the IO-bound
# adapters.
#
# The HTML report is the standard cargo-llvm-cov report (unchanged); it still
# includes test scaffolding, so its totals differ from the gated numbers above.
# The gate thresholds are always printed to the terminal.
#
# Requires:
#   * cargo-llvm-cov     -> cargo install cargo-llvm-cov
#   * llvm-tools-preview -> rustup component add llvm-tools-preview
#   * Docker             -> for the Postgres repository tests (same as `make test`)
#
# Run locally before pushing; wire it into CI to keep the tree covered.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${PROJECT_ROOT}"

COVERAGE_THRESHOLD="${COVERAGE_THRESHOLD:-80}"
CORE_COVERAGE_THRESHOLD="${CORE_COVERAGE_THRESHOLD:-95}"
COVERAGE_DIR="target/coverage"
LCOV_FILE="${COVERAGE_DIR}/lcov.info"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "ERROR: cargo-llvm-cov is not installed." >&2
    echo "" >&2
    echo "  Install it and the LLVM coverage component, then re-run:" >&2
    echo "    rustup component add llvm-tools-preview" >&2
    echo "    cargo install cargo-llvm-cov" >&2
    echo "" >&2
    exit 1
fi

mkdir -p "${COVERAGE_DIR}"

echo "--- cargo llvm-cov (production line coverage: overall >= ${COVERAGE_THRESHOLD}%, core >= ${CORE_COVERAGE_THRESHOLD}%)"
# Run the instrumented test suite once and write the lcov report. Both gates are
# computed from this report below; the standard HTML report is rendered after.
cargo llvm-cov --lcov --output-path "${LCOV_FILE}"

# Computes production-only line coverage from the lcov report. An optional awk
# regex filters the files by path (e.g. '/src/core/').
production_coverage() {
    local filter="${1:-}"
    awk -v filter="${filter}" '
        function test_start(path, line, n) {
            n = 0
            while ((getline line < path) > 0) {
                n++
                if (line ~ /^#\[cfg\(test\)\]/) { close(path); return n }
            }
            close(path)
            return 0
        }
        /^SF:/ {
            file = $0; sub(/^SF:/, "", file)
            if (file ~ /\/tests\// || file ~ /tests\.rs$/ || file ~ /test\.rs$/) {
                skip = 1; b = 0
            } else {
                skip = 0; b = test_start(file)
            }
        }
        /^DA:/ {
            if (skip) next
            if (filter != "" && file !~ filter) next
            split($0, a, ":"); split(a[2], c, ",")
            line = c[1]; cnt = c[2]
            if (b > 0 && line >= b) next
            total++; if (cnt > 0) hit++
        }
        END {
            if (total > 0) printf "%.2f", hit * 100.0 / total
            else print "100.00"
        }
    ' "${LCOV_FILE}"
}

overall_percent="$(production_coverage "")"
core_percent="$(production_coverage '/src/core/')"

# Standard cargo-llvm-cov HTML report (unchanged, includes test scaffolding).
cargo llvm-cov report --html --output-dir "${COVERAGE_DIR}"

echo "coverage: overall (production) ${overall_percent}% (>= ${COVERAGE_THRESHOLD}%)"
echo "coverage: core (src/core/, production) ${core_percent}% (>= ${CORE_COVERAGE_THRESHOLD}%)"

if awk -v pct="${overall_percent}" -v threshold="${COVERAGE_THRESHOLD}" \
    'BEGIN { exit (pct + 0 < threshold) ? 1 : 0 }'; then
    :
else
    echo "ERROR: overall production line coverage is ${overall_percent}% (< ${COVERAGE_THRESHOLD}%)" >&2
    exit 1
fi

if awk -v pct="${core_percent}" -v threshold="${CORE_COVERAGE_THRESHOLD}" \
    'BEGIN { exit (pct + 0 < threshold) ? 1 : 0 }'; then
    :
else
    echo "ERROR: core (src/core/) production line coverage is ${core_percent}% (< ${CORE_COVERAGE_THRESHOLD}%)" >&2
    echo "  the core is pure hexagonal logic and must be unit-tested in isolation; add tests." >&2
    exit 1
fi

echo "coverage: OK"
echo "  lcov: ${LCOV_FILE}"
echo "  html: ${COVERAGE_DIR}/html/index.html"
