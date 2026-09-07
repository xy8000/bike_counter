# 14 - Coverage scan plan

Status: decided

> Superseded by [`15_coverage_thresholds_plan.md`](15_coverage_thresholds_plan.md):
> the single global gate (75%) is replaced by an 80% overall gate plus a 95%
> core (`src/core/`) gate.

## Problem

The repository has strong developer-facing gates — `make check` (rustfmt +
clippy, via [`scripts/fmt-test.sh`](../scripts/fmt-test.sh)) and `make test` — but
**no coverage measurement**. Now that everything compiles and the code has been
refactored, the team wants a coverage scan wired into the repo so future changes
cannot silently reduce test coverage. There is currently no coverage tooling, no
coverage output, and no `agents.md` convention file to enforce it.

## Approach (decision)

Use **`cargo-llvm-cov`** as the coverage tool. It instruments the crate with
native LLVM coverage (stable toolchain, no `nightly`), runs the normal test
suite, and supports a hard line-coverage gate via `--fail-under-lines` plus lcov
and HTML report output. It requires the `llvm-tools-preview` rustup component.

Enforcement is **local-only for now** (no CI): a new [`scripts/coverage.sh`](../scripts/coverage.sh)
gate following the existing `scripts/fmt-test.sh` pattern, wired into the
Makefile as `make coverage`, and documented for agents in a new `agents.md` at
the repo root. The gate fails (non-zero exit) when line coverage falls below a
configurable threshold (`COVERAGE_THRESHOLD`, default **75%**). The current
baseline is **76.68% line coverage** (5121/6678 lines), so 75% was chosen as the
next clean step below the baseline to make the gate pass on the current tree;
raise it as coverage improves.

The full coverage run executes the entire test suite under instrumentation —
including the Postgres repository tests that spin up a test container via Docker
(the same requirement as `make test`).

## Goals

- Add `scripts/coverage.sh`: runs `cargo llvm-cov`, writes `lcov.info` + an HTML
  report under `target/coverage/`, enforces `--fail-under-lines $COVERAGE_THRESHOLD`,
  prints a friendly install hint (`rustup component add llvm-tools-preview`;
  `cargo install cargo-llvm-cov`) and exits non-zero if the tool is missing.
- Add `make coverage` (and a `make coverage-open` helper for the HTML report) to
  the Makefile; extend `.PHONY` and the header/`help` text. `test-all` stays
  `check test` (adding coverage would run the suite twice).
- Add `agents.md` at the repo root enforcing that agents run `make coverage`
  (plus `make check` and the tests) before finishing changes, and that every
  change gets a plan file in `./plans/` registered in `plans/README.md`.
- Add coverage artifacts (`*.profraw`, `lcov.info`, `coverage/`) to `.gitignore`.
- Calibrate the default threshold against the measured baseline so the gate
  passes on the current tree. Measured baseline: **76.68% lines**; default
  `COVERAGE_THRESHOLD` set to **75%**.
- Document the coverage target in `README.md` (Running tests) and in this plan.

## Deliverables

- [x] `scripts/coverage.sh` — the coverage gate
- [x] `Makefile` — `coverage` + `coverage-open` targets, `.PHONY`, header/`help`
- [x] `agents.md` — coverage + plan-file conventions for agents
- [x] `.gitignore` — coverage artifacts
- [x] `README.md` — Running tests section update
- [x] Baseline measured (**76.68% lines**); threshold calibrated to **75%**;
      gate green (`make check`, `make coverage`, `make test-rest`, and
      `make test` where Docker is available)

## Out of scope

- GitHub Actions / CI workflow (a natural follow-up once the local gate is in).
- Coverage exclusions / per-file ignore lists (e.g. `main.rs` glue) — only added
  later if the threshold makes them necessary.
- `rust-toolchain.toml` toolchain pinning — not introduced here.
