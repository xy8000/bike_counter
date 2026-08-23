# Agents

Conventions and required workflows for AI agents and contributors working in
this repository. **Read this file before making any change.**

## Workflow

1. **Write a plan first.** Before implementing anything, create or update a
   numbered plan document in [`plans/`](plans) — `plans/NN_<topic>_plan.md`,
   following the existing format — and register it in
   [`plans/README.md`](plans/README.md). This is **mandatory**: every change
   gets a plan file, no exceptions.
2. **Implement** the change, keeping it scoped to the plan.
3. **Run the gates** before finishing (see below) — all must pass.
4. **Update the docs** the change touches ([`README.md`](README.md),
   [`ToDo.md`](ToDo.md), and the plan file itself).
5. Do not risk wasting tokens for commands. Use tail / head when possible

## Required gates (run before finishing any change)

| Command | Purpose |
|---|---|
| `make check` | `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` |
| `make test` | Full test suite (Postgres repository tests spin up a Docker test container) |
| `make test-rest` | REST endpoint tests only (in-memory mocks, no Docker required) |
| `make coverage` | **Coverage gate — fails when overall *production* line coverage is below `COVERAGE_THRESHOLD` (default 80%) or the core (`src/core/`) is below `CORE_COVERAGE_THRESHOLD` (default 95%)** |

### Coverage

`make coverage` runs [`scripts/coverage.sh`](scripts/coverage.sh), which executes
the whole test suite with LLVM instrumentation (`cargo-llvm-cov`) and **fails the
build when overall production line coverage drops below `COVERAGE_THRESHOLD`
(default 80%)** or when the **core** (`src/core/`, the domain + application
layer) drops below `CORE_COVERAGE_THRESHOLD` (default 95%). Both thresholds are
measured on **production code only**: lines inside `#[cfg(test)]` modules and
standalone test files are excluded, so test scaffolding can never inflate the
number. The core is pure hexagonal logic and is expected to be fully unit-tested
in isolation with in-memory mocks, so its bar is higher than the adapters'. New
code must keep coverage at or above the thresholds — prefer adding tests for new
behavior over lowering them.

Install the tooling once:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
```

Notes:

- The full coverage run needs Docker (Postgres test container), like `make test`.
- The standard cargo-llvm-cov report is at `target/coverage/html/index.html`
  (open it with `make coverage-open`); it includes test scaffolding, so its
  totals differ from the **production-only** gate numbers printed in the
  terminal. The lcov data is at `target/coverage/lcov.info`.
- For a one-off run use `COVERAGE_THRESHOLD=<percent>` and/or
  `CORE_COVERAGE_THRESHOLD=<percent> make coverage` — never commit a lowered
  threshold.

## Definition of done

- [ ] Plan file in [`plans/`](plans) updated and registered in
      [`plans/README.md`](plans/README.md)
- [ ] `make check` green
- [ ] `make test` and/or `make test-rest` green
- [ ] `make coverage` green (coverage at/above the threshold)
- [ ] `README.md` / `ToDo.md` / plan docs updated as needed
