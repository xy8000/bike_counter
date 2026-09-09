#!/usr/bin/env bash
# Reduces a cargo-llvm-cov lcov report to PRODUCTION-ONLY line data.
#
# cargo-llvm-cov's raw lcov also records the lines of #[cfg(test)] modules and
# of standalone test files — all of them are "hit" by the test run, so uploading
# the raw file would inflate a Codecov report. This filter drops that test
# scaffolding so the uploaded numbers match the production-only gate that
# scripts/coverage.sh enforces locally (overall >= 80 %, core >= 95 %):
#   * standalone test files (paths containing /tests/, or named tests.rs /
#     test.rs) are removed entirely, and
#   * every line from the first "#[cfg(test)]" marker of a file onwards is
#     removed.
#
# LF/LH counters are recomputed from the surviving DA lines so the output is a
# self-consistent lcov file (function/branch records are dropped with the
# scaffolding — Codecov tracks line coverage here).
#
# Not part of `make coverage` (that gate is unchanged); CI uses this script only
# to build the upload artifact.
#
# Usage: ./scripts/lcov-production-only.sh <input.lcov> > lcov.production.info
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <input.lcov>" >&2
    exit 1
fi

INPUT="$1"

awk '
    # Returns the 1-based line number of the first "#[cfg(test)]" marker in the
    # given source file (0 when the file has none). Mirrors the same helper in
    # scripts/coverage.sh so the two never drift apart.
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
            skip = 1
            next
        }
        skip = 0
        block = test_start(file)
        in_record = 1
        lines = 0
        hits = 0
        print $0
        next
    }
    skip { next }
    in_record && /^DA:/ {
        split($0, a, ":"); split(a[2], c, ",")
        if (block > 0 && (c[1] + 0) >= block) next
        print $0
        lines++
        if (c[2] + 0 > 0) hits++
        next
    }
    in_record && /^end_of_record/ {
        print "LF:" lines
        print "LH:" hits
        print $0
        in_record = 0
        next
    }
' "${INPUT}"
