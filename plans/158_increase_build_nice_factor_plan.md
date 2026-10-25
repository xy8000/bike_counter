# 158 - Increase nice-factor for build processes

Status: implemented

## Goal

Lower the CPU priority of the repo's heavy build/test steps by one nice level
(`nice -n 1`) so a local `cargo`/`npm` build no longer starves the rest of the
machine (GitHub issue #39).

## Changes

- [`Makefile`](Makefile): introduce `NICE := nice -n 1` and wrap the
  `build`, `test`, `test-rest`, `test-unit`, `test-unit-coverage`, `coverage`
  (frontend step) and `frontend-build` targets.
- [`scripts/fmt-test.sh`](scripts/fmt-test.sh): run `cargo clippy` under
  `nice -n 1`.
- [`scripts/coverage.sh`](scripts/coverage.sh): run both `cargo llvm-cov`
  invocations under `nice -n 1`.

Docker-based builds (`docker compose up --build`, the `tiles` targets) are left
unchanged: the heavy work happens inside the build container, where the host's
`nice` value does not propagate.

## Definition of done

- [x] Build/test/coverage commands run one nice level lower
- [x] `make -n` dry-run confirms the wrapping
