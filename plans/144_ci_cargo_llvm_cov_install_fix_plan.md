# 144 - Fix CI: install cargo-llvm-cov before the coverage gate

Status: implemented

## Problem

The CI job **"Backend coverage (Rust)"** fails at
`./scripts/coverage.sh` ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml:62))
with:

```
ERROR: cargo-llvm-cov is not installed.
```

## Root cause

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml:52) installs the tool
with `taiki-e/install-action`. That action takes the tool to install either from
its `tool:` input — which is `required: true` — or from a tool-named action ref
(the shorthand `taiki-e/install-action@cargo-llvm-cov`, which is what
[`plans/121`](121_codecov_github_actions_plan.md:117) originally used).

A later commit pinned the action to a full commit SHA for supply-chain safety
([`plans/126`](126_sonar_findings_plan.md:18) /
[`plans/132`](132_sonar_security_findings_fix_plan.md:1)) but did not re-add the
`tool:` input. With a non-tool ref and no `tool:`, the action had nothing to
install, so `cargo-llvm-cov` was absent when the coverage step ran.

## Changes

1. [`.github/workflows/ci.yml`](../.github/workflows/ci.yml:52): add
   `with:\n tool: cargo-llvm-cov` to the SHA-pinned `taiki-e/install-action`
   step, keeping the SHA pin (the tool is now named explicitly).

## Verification

- Workflow YAML validated (actionlint).
- Step now passes the required `tool:` input while remaining SHA-pinned.

## Outcome

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml:52) now passes
`tool: cargo-llvm-cov` to the SHA-pinned `taiki-e/install-action` step, so the
action installs the tool instead of no-oping. The SHA pin is preserved, keeping
the supply-chain hardening from plans 126/132 while restoring the tool identity
the shorthand ref used to provide.

## Definition of done

- [x] [`.github/workflows/ci.yml`](../.github/workflows/ci.yml:52) passes `tool: cargo-llvm-cov`
- [x] workflow YAML valid (actionlint, exit 0)
- [x] plan file status/checklist kept current
