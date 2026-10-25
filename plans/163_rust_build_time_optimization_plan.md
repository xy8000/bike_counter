# 163 - Rust build-time reduction

Status: implemented

## Problem

The Rust backend ([`backend/`](../backend)) is slow to build from scratch and its
build artifacts are enormous. The question was: *is there anything to cut down the
Rust build time?*

## Findings (measured in this workspace, 16 cores, rustc 1.94.0)

Warm **incremental** builds were already fast: a full crate rebuild (196 files,
62 512 LOC) after `touch src/main.rs` took **~11 s** (`cargo build`) and **~12 s**
(`cargo check`). The real cost sat in cold builds, link weight and disk:

- `backend/target` was **51 GB** (42 GB `debug/`, incl. 20 GB `incremental/`;
  9.1 GB of a second `llvm-cov-target/` tree).
- The debug binary was **412 MB**: `[profile.*]` was completely untuned, so every
  crate — ours *and* all dependencies — carried full `debug = 2` DWARF.
- 461 locked packages, 250 build scripts, and several **C toolchain builds**
  (`zstd-sys`, `bzip2-sys`, `lzma-sys`, `xz`) that the crate never uses:
  - `parquet` was pulled with its default features (`brotli`, `lz4`, `zstd`,
    `flate2-zlib-rs`, `simdutf8`) although
    [`opendata_file_generator.rs`](../backend/src/adapter/driven/opendata_file_generator.rs:148)
    only ever writes `Compression::SNAPPY`;
  - `zip` was pulled with its defaults (`aes-crypto`, `bzip2`, `deflate64`,
    `lzma`, `ppmd`, `time`, `zstd`, `xz`) although the Münster importer only
    extracts plain deflate archives;
  - `arrow` was pulled with its `csv`/`json`/`ipc` defaults although only
    `arrow::array`, `arrow::datatypes` and `arrow::record_batch` are used.
- `sha2` was duplicated (our direct `0.11.1` vs. the `0.10.9` that `rust-s3`
  already resolves), forcing a second copy of the crypto stack.

## Decision: configuration only, no new build modes

The goal was a smaller build, not a bigger build system. Every change below is a
value in [`backend/Cargo.toml`](../backend/Cargo.toml) — there is no new feature
flag, no new environment variable and no extra command for anyone to remember.

**Explicitly rejected:** making `openssl/vendored` an opt-in cargo feature so
local/CI builds could link the system OpenSSL (that would have removed the
`openssl-src` C build from the everyday loop). It would have required passing
`--features vendored-openssl` in [`backend/Dockerfile`](../backend/Dockerfile)
and would have made the Docker build differ from a local build — a new build mode
to remember. Reverted; `openssl` stays vendored everywhere, exactly as before.

## Changes

- [`backend/Cargo.toml`](../backend/Cargo.toml:70): `[profile.dev]` and
  `[profile.test]` set `debug = "line-tables-only"` (our own backtraces keep
  file/line info) plus `package."*" debug = false` (dependencies carry no DWARF,
  which was the bulk of the 42 GB). Both profiles are tuned because `cargo test`
  and `cargo llvm-cov` use `test` and do not inherit `package` overrides.
  `release` is untouched so shipped binaries keep their symbols.
- [`backend/Cargo.toml`](../backend/Cargo.toml:33): `parquet` with
  `default-features = false, features = ["arrow", "snap"]`.
- [`backend/Cargo.toml`](../backend/Cargo.toml:55): `zip` with
  `default-features = false, features = ["deflate-flate2"]`.
- [`backend/Cargo.toml`](../backend/Cargo.toml:17): `arrow` with
  `default-features = false`.
- [`backend/Cargo.toml`](../backend/Cargo.toml:43): `sha2` `0.11.1` → `0.10.9`
  (identical `Sha256::digest` API, matches `rust-s3`).
- [`backend/Cargo.lock`](../backend/Cargo.lock:1): regenerated.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md:78): three-line note on the tuning.

## Measured effect

| Metric | Before | After |
| --- | --- | --- |
| Locked packages | 461 | **437** |
| `zstd-sys` / `bzip2-sys` / `lzma-sys` / `xz` (C builds) | present | **gone** |
| `brotli`, `lz4_flex`, `zopfli`-by-zip, `aes`/`hmac`/`pbkdf2`, `arrow-csv`, `arrow-json` | present | **gone** |
| Dev binary size | 412 MB | **80 MB** |
| Warm `cargo build` after `touch src/main.rs` | 11.3 s | **5.8 s** |
| `cargo check --all-targets` (deps warm) | 12.2 s | 12.2 s (unchanged) |
| Whole test suite (incl. Postgres container) | — | **847 passed** in ~44 s |
| Coverage gate (production lines) | 80 % / 95 % | **87.23 % overall, 97.14 % core** |

`zopfli` is still in the graph: it is a **build**-dependency of
`utoipa-swagger-ui` (for its UI asset zip), not a runtime dependency, so it is
compiled once for the host and cannot be trimmed from here.

## Notes for future work (not done here)

- `backend/target` still holds stale `debug = 2` artifacts from before the
  profile change; Cargo never garbage-collects them. A one-off
  `cargo clean --manifest-path backend/Cargo.toml` reclaims the old ones — the
  directory then stays a few GB instead of tens of GB.
- `zip` is now built with the `flate2` deflate backend only. If a future importer
  needs zstd/lzma/bzip2 archives, add that feature to the one line instead of
  restoring all defaults.
- `cargo llvm-cov` builds into a second tree (`backend/target/llvm-cov-target`);
  it now inherits the trimmed `test` profile but is still a separate build.
- `syn` 2 + 3, `rand` 0.8/0.9/0.10 and `phf` 0.11/0.12/0.13 are duplicated by
  transitive majors (not by this crate); only upstream releases can remove them.
- BuildKit cache mounts for `target/` would speed up local Docker rebuilds after
  a `Cargo.lock` change, but they would also remove the dependency artifacts from
  the image layer and therefore defeat the `type=gha` layer cache used by
  [`release.yml`](../.github/workflows/release.yml). The existing stub-dependency
  layer is kept for that reason.

## Definition of done

- [x] Plan file updated
- [x] `backend/Cargo.toml` profile + dependency-feature changes applied
- [x] `backend/Cargo.lock` regenerated and consistent with `--locked` builds
- [x] `backend/Dockerfile` unchanged (no new feature flag to remember)
- [x] `make check` green (fmt + clippy + Prettier + `cargo audit`)
- [x] `make test` and/or `make test-rest` green (847 / 130 tests)
- [x] `make coverage` green (backend thresholds + frontend Vitest gate)
- [x] `make test-playwright` green when the change touches the frontend UI (not applicable: backend build config only)
- [x] `README.md` and plan docs updated as needed ([`CONTRIBUTING.md`](../CONTRIBUTING.md:78), this plan)
