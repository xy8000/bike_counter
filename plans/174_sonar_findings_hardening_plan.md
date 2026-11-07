# 174 - Sonar findings: file permissions, cognitive complexity, nested ternary, `!important`

Status: implemented

## Context

SonarQube reports seven findings on the current branch. None of them is a
functional bug, but all are hygiene rules that must be resolved by fixing the
underlying cause rather than by suppression:

| # | File | Rule | Finding |
|---|---|---|---|
| 1 | [`tiles_init/mod.rs`](backend/src/adapter/driven/tiles_init/mod.rs:336) | rust:S2612 | the cached `pmtiles` CLI is chmod-ed `0o755`, i.e. world-executable |
| 2 | [`bonn_opendata/parsing.rs`](backend/src/adapter/driven/bonn_opendata/parsing.rs:356) | rust:S3776 | `parse_yearly_hourly_csv` cognitive complexity 43 (> 30) |
| 3 | [`core/application/station_analytics/graphs.rs`](backend/src/core/application/station_analytics/graphs.rs:426) | rust:S3776 | `period_data` cognitive complexity 61 (> 30) |
| 4 | [`VersionDialog.tsx`](frontend/src/features/header/VersionDialog.tsx:22) | typescript:S3358 | nested ternary for the backend version label |
| 5-7 | [`index.css`](frontend/src/index.css:108) | css:S4658 | 14 `!important` declarations (station marker + MapLibre overlay theming) |

## Goals

- Tighten the CLI permissions to owner-only and audit the surrounding file and
  directory creation.
- Split both complex functions into named helpers so each stays well below 30
  without changing behaviour or their observable output.
- Replace the nested ternary with a named helper.
- Remove every `!important` by resolving the cascade conflict: the MapLibre
  stylesheet is imported into a low-priority `vendor` cascade layer, so the app
  rules win on layer order instead of on `!important`.

## Non-goals

- No change to the computed graph values, the imported rows, the REST payloads
  or the rendered map/marker visuals.
- No new thresholds, no `#[allow]`/`eslint-disable`/`sonar-exclusion` and no
  weakening of the existing gates.

## 1. Cached `pmtiles` CLI permissions (rust:S2612)

`TilesInit::ensure_cli` downloads the release tarball and then calls
`make_executable`, which chmod-ed the binary `0o755`. The only consumer is the
backend process itself (`USER app`, uid 1000 in
[`backend/Dockerfile`](backend/Dockerfile:49)), so world/group execution is not
needed.

- `make_executable` now sets `0o700` (owner `rwx`, no group/world bits) and its
  doc comment states the least-privilege rationale.
- Audit of the remaining filesystem work in the same module:
  - `tiles_dir` keeps plain `create_dir_all`: `./tiles` is a bind mount that
    nginx also serves read-only ([`docker-compose.yml`](docker-compose.yml:127)),
    so it must stay traversable/readable; nothing world-writable is requested.
  - the CLI cache directory is created with an explicit `0o700` mode
    (`DirBuilder::mode`), so the download target is not even listable by other
    users, matching the binary's own mode.
  - the temporary `pmtiles.tar.gz` is written with the process umask and deleted
    immediately after extraction; it holds no secrets.
- Tests: a new `make_executable_grants_owner_execute_only` asserts the exact
  mode, and the existing fake-CLI test helpers use `0o700` as well.

## 2. `parse_yearly_hourly_csv` complexity (rust:S3776)

The function did four jobs in one nested loop (header mapping, timestamp
parsing, per-column value emission, message emission). It is split into
single-purpose helpers that all live next to it:

- `emit_csv_message` — one `provider_event_occurred` call site.
- `yearly_column_channels` — maps the `Time` header row to one
  `Option<channel>` per column and warns about unmapped indexed columns.
- `yearly_row_timestamp` — skips blank lead cells, emits the "unparsable
  timestamp" debug message and returns the UTC timestamp.
- `push_yearly_values` — emits one `WideRow` per mapped, non-empty,
  integer-parsable cell and debug-logs non-integer cells.

The public signature, the returned rows, the skip rules and every provider
message text stay identical; the existing `parse_yearly_hourly_csv_*` tests
cover all four paths.

## 3. `period_data` complexity (rust:S3776)

`period_data` mixed six independent derivations. The extraction keeps the
control flow, only naming the steps:

- `established_group_ids` — the Bike-Trends like-for-like group set (incl. the
  `introduced_after` filter).
- `established_rows` — the per-channel row filter used by both periods.
- `fold_series` — per-channel buckets folded into one aggregate series.
- `weekday_groups_by_channel` / `hour_groups_by_channel` — the per-group
  weekday/hour radars.
- `fold_weekdays` (existing) / `fold_hours` — the aggregate radars.
- `series_and_pie_by_channel` — per-group series plus the pie totals.
- `group_graph` — the per-group output record (zero-filling + radar selection).

`period_data` itself keeps the window/period selection logic and the final
`PeriodData` assembly, well under the limit.

## 4. `VersionDialog` nested ternary (typescript:S3358)

`backendVersionLabel(version, error)` replaces the nested conditional
expression: `v<version>` when known, otherwise `Unavailable` on error and
`Loading…` while the request is in flight. The component's rendered output is
unchanged; the existing `VersionDialog` tests still cover all three states.

## 5. `!important` removal (css:S4658)

`maplibre-gl.css` is imported from [`map.tsx`](frontend/src/lib/map.tsx:1) and
lands **unlayered**, so it outranks the app's `@layer` rules regardless of
source order — the reason the overlay rules were forced with `!important`.

The fix moves the vendor sheet into its own cascade layer:

- [`index.css`](frontend/src/index.css:1) imports
  `maplibre-gl/dist/maplibre-gl.css` with `layer(vendor)` **first**, so `vendor`
  is the lowest-priority layer (declared before Tailwind's
  `theme, base, components, utilities`).
- [`map.tsx`](frontend/src/lib/map.tsx:1) drops the JS-side import (the entry
  stylesheet owns it now).
- all 14 `!important` declarations are removed: the popup, tip, control group
  and attribution rules are unlayered and therefore always beat the `vendor`
  layer; `.station-marker` (0,1,0) already outranks Tailwind's preflight `img`
  (0,0,1) inside `@layer base`, and the `.station-marker--large` (0,1,0, later)
  override keeps winning because no selector gained specificity.
- the explanatory comments are rewritten to document the layer ordering.

The map e2e suite (`map.spec.ts`, `dark-mode.spec.ts`, marker/flag checks) and
the unit tests exercise the rendered result after the change.

## Definition of done

- [x] plan file added and kept current
- [x] CLI cached with `0o700` (owner-only) and the cache directory created
      `0o700`; related creations audited and documented
- [x] `make_executable_grants_owner_execute_only` test added (plus
      `create_private_dir_grants_owner_only`)
- [x] `parse_yearly_hourly_csv` split into helpers, complexity below 30,
      behaviour and messages unchanged
- [x] `period_data` split into helpers, complexity below 30, output unchanged
- [x] `VersionDialog` nested ternary replaced by a named helper
- [x] all 14 `!important` removed via the `vendor` cascade layer; no visual
      regression in the map overlays/markers
- [x] `make check`, `make test` (930 passed), `make test-rest` (158 passed) and
      `make coverage` (overall 89.73 %, core 97.61 %, frontend 96.57 %) green
- [x] frontend build (`tsc && vite build`), unit tests (584 passed) and
      `make test-playwright` (76 passed) green
- [x] docs touched by the change updated (this plan + plan 173); the change is
      internal, so no README/OpenAPI edit was needed

## Implementation record

### 1. Permissions

- [`make_executable`](backend/src/adapter/driven/tiles_init/mod.rs:336) now sets
  `0o700`; the doc comment records why group/world bits are unnecessary.
- New [`create_private_dir`](backend/src/adapter/driven/tiles_init/mod.rs:352)
  creates the CLI cache with `DirBuilder::mode(0o700)`; `ensure_cli` uses it
  instead of `create_dir_all`. The tiles directory keeps `create_dir_all` on
  purpose (nginx serves it read-only from the same bind mount).
- Audit result: the tarball is written with the process umask and deleted right
  after extraction; no other file or directory in the module requests
  world-writable/world-executable bits.
- Tests: `make_executable_grants_owner_execute_only`,
  `create_private_dir_grants_owner_only`; the fake-CLI test helpers now share
  `write_fake_binary` (also `0o700`), so no test writes a `0o755` binary either.

### 2. `parse_yearly_hourly_csv`

Extracted `emit_csv_message`, `yearly_column_channels`, `yearly_row_timestamp`
and `push_yearly_values`; the public signature and every skip/emit rule are
unchanged (the four existing `parse_yearly_hourly_csv_*` tests pass). Clippy's
`cognitive_complexity` lint no longer reports the function.

### 3. `period_data`

Extracted `established_group_ids`, `established_rows`, `fold_series`,
`weekday_groups_by_channel`, `hour_groups_by_channel`, `fold_hours`,
`series_by_channel` and the `GroupSources` struct (`GroupSources::graph`).
Two deliberate simplifications, both output-identical:

- the previous period no longer builds an unused pie map;
- `pie_total` is the sum of the group's current series, exactly the value the
  removed `pie_by_group` accumulator held (it summed the same rows).

Clippy's `cognitive_complexity` lint no longer reports the function; the graph
unit tests and the summary/detail REST tests still pass.

### 4. `VersionDialog`

`backendVersionLabel(version, error)` replaces the nested ternary; the three
rendered states are unchanged (`VersionDialog.test.tsx` passes).

### 5. `!important`

`index.css` imports `maplibre-gl/dist/maplibre-gl.css` with `layer(vendor)` as
the first rule, [`map.tsx`](frontend/src/lib/map.tsx:1) no longer imports it
from JS, and all 14 `!important` are gone. The production build emits
`@layer properties, vendor, theme, base, utilities` in that order with the
themed `.maplibregl-*` rules unlayered, so they win by cascade-layer order; the
only `!important` left in the bundle are maplibre's own
`.maplibregl-pseudo-fullscreen` ones, which touch none of the themed properties.
`dark-mode.spec.ts`, `map.spec.ts` and the marker/flag e2e checks pass.
