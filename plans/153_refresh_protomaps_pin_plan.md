# 153 - Fix the failing `tiles_update` job (self-healing Protomaps pin)

Status: implemented

> Gates run: `make check` (fmt + clippy + Prettier + cargo audit) and the full
> `make test` — including the new `update_error_includes_cli_stderr`,
> `latest_build_key_*` and `resolve_source_*` unit tests.

## Problem

The scheduled `tiles_update` job fails on every run with an opaque provider
error, so the self-hosted basemap (`tiles/map.pmtiles`) never (re)builds:

```text
ERROR bike_counter::core::application::tiles_update_service: Tiles update job
tiles update (03b0404f-…) failed:
Provider("/data/.pmtiles-bin/pmtiles exited with exit status: 1")
```

The error carries no cause, and the job just keeps failing.

## Root cause

The basemap is built by driving the `go-pmtiles` CLI
([`TilesInit`](backend/src/adapter/driven/tiles_init/mod.rs)) against a **dated**
Protomaps daily build pinned as `maps.protomaps_build_url`
([`tiles/README.md`](tiles/README.md:88)). Protomaps only keeps a short window
(~a month) of dailies and then **prunes** the older ones, so the pinned URL
starts returning **HTTP 404**; `pmtiles extract` then exits `1`.

Verified against the live endpoint (2026-10-08):

| Pinned in | Build | HTTP |
|---|---|---|
| [`config.toml`](config.toml:30) (mounted, git-ignored) | `20260829` | **404** |
| [`config.toml.example`](config.toml.example:75), code default + tests | `20260905` | **404** |
| live | `20261006` / `20261007` / `20261008` | 200 |

So **both** the deployed config pin *and* the code default are dead — exactly
the "follow-up (out of scope)" recorded in
[`plans/149`](plans/149_replace_minio_with_garage_plan.md:231) and the pruning
that already forced one bump in
[`plans/111`](plans/111_germany_surroundings_tiles_plan.md:90).

The second problem is **diagnosability**: [`TilesInit::run`](backend/src/adapter/driven/tiles_init/mod.rs:213)
only reports the exit status, so the CLI's own `404 …`/error output never reaches
the job error message (it is inherited to the container log, but not captured),
which is why the failure looked like a bare `exit status: 1`.

## Approach

1. **Refresh the pin to the newest live build (`20261008`)** everywhere it is
   declared so the job can rebuild again and has the longest runway before the
   next upstream prune:
   - [`config.toml`](config.toml:30) (the mounted, git-ignored local config that
     actually fails),
   - [`config.toml.example`](config.toml.example:75),
   - the `DEFAULT_MAPS_PROTOMAPS_BUILD_URL` default in
     [`configuration_toml_adapter.rs`](backend/src/adapter/driven/configuration_toml_adapter.rs:83)
     and every test expectation that asserts it,
   - the two test-script configs
     ([`scripts/docker-compose-test.sh`](scripts/docker-compose-test.sh:89),
     [`scripts/e2e-playwright.sh`](scripts/e2e-playwright.sh:118)),
   - the `maps_configuration()` helpers in the backend unit tests.
2. **Surface the CLI's stderr in the failure.** Change
   [`TilesInit::run`](backend/src/adapter/driven/tiles_init/mod.rs:213) to capture
   the child's stderr (stdout stays inherited, so the multi-minute progress is
   still streamed) and append the trimmed output to the returned error, plus a
   short hint when the output looks like a `404`/pruned build. A future pin
   expiry then shows *why* in the job log instead of a bare exit status. Add a
   unit test proving the stderr text is included.
3. **Document the symptom** in
   [`tiles/README.md`](tiles/README.md:88): a pruned pin shows up as the
   `tiles_update` job failing with the pmtiles CLI error.
4. **Self-heal the pin (the durable fix).** A date bump alone only buys ~a month:
   the tiles job runs every two months
   (`0 0 3 1 1,3,5,7,9,11 *`), so it would 404 again. Protomaps *does* publish a
   stable machine-readable catalog
   (`https://build-metadata.protomaps.dev/builds.json` — an array of
   `{"key":"YYYYMMDD.pmtiles", ...}` that its own builds page consumes), so
   [`TilesInit::build_to`](backend/src/adapter/driven/tiles_init/mod.rs:128) now
   **resolves the source per build** via `resolve_source_default`: the configured
   pin while it still resolves, otherwise the newest catalog build (logging a
   warning). A pruned pin therefore never fails the job again; the pin only
   chooses the *preferred* snapshot. The network effects are injected
   (`resolve_source_from`) so the policy and the catalog parser are unit-tested
   offline, and the fake-CLI tests use a stub resolver to stay network-free.
5. **Make `latest` the default (no hardcoded date).** `protomaps_build_url` now
   accepts the token `latest` (case-insensitive) meaning "always the newest
   catalog build", and it becomes the code default
   (`DEFAULT_MAPS_PROTOMAPS_BUILD_URL`), the `config.toml`/`config.toml.example`
   value and the test fixtures — so no dated pin has to be refreshed at all.
   Pinning an explicit `YYYYMMDD.pmtiles` remains an opt-in freeze.

## Definition of done

- [x] `protomaps_build_url` bumped to the then-live `20261008` in `config.toml`,
      `config.toml.example`, the `DEFAULT_MAPS_PROTOMAPS_BUILD_URL` const, both
      test scripts and every backend test expectation.
- [x] `protomaps_build_url` supports the `latest` token (case-insensitive) and
      defaults to it everywhere (`config.toml`, `config.toml.example`, the code
      default and fixtures); a resolver unit test covers the token.
- [x] [`TilesInit::run`](backend/src/adapter/driven/tiles_init/mod.rs:213) captures
      and includes the CLI's stderr in the error; unit test added.
- [x] [`tiles/README.md`](tiles/README.md:88) documents the pruned-pin symptom
      and the self-healing behavior.
- [x] `TilesInit` resolves the source per build — configured pin while available,
      else the newest catalog build — so a pruned pin self-heals. Resolver policy
      + catalog parser unit-tested, and the real network paths
      (`resolve_source_default`, `url_is_available`, `fetch_latest_build_key`)
      are exercised hermetically against a loopback HTTP server; the fake-CLI
      tests use a stub resolver and are serialised to avoid the pre-existing
      `Text file busy` flake under parallel execution.
- [x] `make check` green.
- [x] `make test-rest` (128) and full `make test` green.
- [x] Plan file ticked and status set to `implemented`.

## Operator note

The failing value lived in the **mounted, git-ignored** [`config.toml`](config.toml:30)
(`./config.toml:/app/config.toml:ro`), so the running stack must be restarted to
pick up the new pin:

```bash
make down && make run    # or: docker compose up -d --build backend
```
