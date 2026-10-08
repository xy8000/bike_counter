# 153 - Refresh the pinned Protomaps build (fix the failing `tiles_update` job)

Status: implemented

> Gates run: `make check` (fmt + clippy + Prettier + cargo audit), `make
> test-rest` (128), and the full `make test` (823, incl. the new
> `update_error_includes_cli_stderr`).

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
   `tiles_update` job failing with the pmtiles CLI error, and the fix is the same
   bump procedure.

Out of scope: automatic resolution of the "latest" build (Protomaps exposes no
stable machine-readable builds endpoint; the pin is deliberately reproducible —
see [`plans/65`](plans/65_bundle_tiles_init_into_backend_image_plan.md:118)).

## Definition of done

- [x] `protomaps_build_url` bumped to `20261008` in `config.toml`,
      `config.toml.example`, the `DEFAULT_MAPS_PROTOMAPS_BUILD_URL` const, both
      test scripts and every backend test expectation.
- [x] [`TilesInit::run`](backend/src/adapter/driven/tiles_init/mod.rs:213) captures
      and includes the CLI's stderr in the error; unit test added.
- [x] [`tiles/README.md`](tiles/README.md:88) documents the pruned-pin symptom.
- [x] `make check` green.
- [x] `make test-rest` green (and `make test` where Docker is available).
- [x] Plan file ticked and status set to `implemented`.

## Operator note

The failing value lived in the **mounted, git-ignored** [`config.toml`](config.toml:30)
(`./config.toml:/app/config.toml:ro`), so the running stack must be restarted to
pick up the new pin:

```bash
make down && make run    # or: docker compose up -d --build backend
```
