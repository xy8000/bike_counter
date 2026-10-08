# 152 - Eco-Counter web adapter: parse the flight payload from the HTML document

Status: implemented

## Problem

The `eco_counter_web_http_provider` data sources (Hessen Mobil, Düsseldorf,
Köln) fail every import with

```
Provider("Unreachable(\"eco-counter web: station list fetch failed: http status: 404\")")
```

The Düsseldorf source shows "Updated never", and the other web tenants are
equally broken.

### Root cause (verified live, 2026-10-08)

Eco-Counter reworked the `*.eco-counter.com` dashboards. Requesting the tenant
root with the `RSC: 1` header now returns a **307 redirect** to the
tenant-prefixed path and that route answers **404**:

```text
GET /?granularity=P1D&year=2026   (RSC: 1)
  -> 307 Location: https://duesseldorf.eco-counter.com/duesseldorf?granularity=P1D&year=2026&_rsc
  -> 404 text/x-component (5738 B Next.js not-found flight payload)
```

The doc comment in [`client.rs`](../backend/src/adapter/driven/eco_counter/scraping/client.rs:15)
still claims "the `RSC: 1` header is what makes Next.js answer with the Flight
payload" — that is no longer true for these tenants, and the header is exactly
what triggers the redirect chain ending in the 404.

The **plain HTML** document (no `RSC: 1` header) is served fine (`200`,
`text/html`) and **inlines the same React Server Components Flight stream** as
escaped JSON inside `self.__next_f.push([1,"<flight text>"])` script calls:

- `GET {root}/?granularity=P1D&year={year}` → HTML whose inlined flight stream
  still carries the tenant's station list under `"sites":[...]` (13 bicycle
  sites for Düsseldorf, 549 for Hessen).
- `GET {root}/site/{id}?granularity=P1D&year={year}` → HTML whose inlined flight
  stream carries the daily series under `"chartData":[...]`
  (`{"travelMode":"bike","data":[{"timestamp":"2026-01-01T00:00:00+01:00","traffic":{"counts":153}}, …]}`,
  280 points for the current year of a Düsseldorf site).

The JSON shapes the parsers already understand (`sites[]`, `chartData[]`,
`attributes.address*`, `location.lat/lon`) are unchanged — only their
**transport** changed.

## Fix

1. **Stop sending `RSC: 1`.** [`fetcher.rs`](../backend/src/adapter/driven/eco_counter/scraping/fetcher.rs)
   fetches the normal server-rendered HTML document (a browser-like
   `User-Agent` stays; the redirect chain is gone).
2. **Decode the inlined flight stream before parsing.** Add a
   `flight_stream(body)` helper in
   [`parsing.rs`](../backend/src/adapter/driven/eco_counter/scraping/parsing.rs)
   that:
   - returns the body unchanged when it is already a bare flight payload
     (backward compatible with the unit-test fixtures and any tenant that ever
     serves the `text/x-component` body directly);
   - otherwise scans every `self.__next_f.push([1,` occurrence, JSON-decodes the
     string argument (`serde_json::from_str::<String>`, which undoes the `\"`
     and `\n` escaping) and concatenates all chunks **in document order** — a
     record may be split across two `push` calls, so concatenation must happen
     before the existing line-oriented `<id>:<json>` scan.
   [`first_field`](../backend/src/adapter/driven/eco_counter/scraping/parsing.rs:92)
   runs on that concatenated stream, so `parse_site_list` / `parse_daily_series`
   are unchanged.

## File changes

Modified:

- `backend/src/adapter/driven/eco_counter/scraping/fetcher.rs` — drop the
  `RSC: 1` header; update the module docs.
- `backend/src/adapter/driven/eco_counter/scraping/parsing.rs` — add
  `flight_stream` + use it in `first_field`; update docs.
- `backend/src/adapter/driven/eco_counter/scraping/tests.rs` — new tests for
  HTML-wrapped and chunk-split payloads.
- `backend/src/adapter/driven/eco_counter/scraping/README.md` — describe the new
  HTML/flight transport, note the `RSC: 1` 404.
- `backend/src/adapter/driven/eco_counter/README.md` and `mod.rs` — wording:
  the flight payload is inlined in the HTML document.
- `plans/152_eco_counter_web_html_flight_payload_plan.md` — this plan.

No DB migration, no config change and no change to the provider seam.

## Testing

- Unit: `flight_stream` extracts a station list / daily series wrapped in a
  minimal `self.__next_f.push([1,"…"])` HTML document.
- Unit: a flight stream split across two `push` calls is concatenated before
  parsing.
- Unit: a bare `<id>:<json>` payload (no marker) is returned unchanged, so the
  existing fixtures keep passing.
- Live sanity: `curl` the three tenants as plain HTML and confirm `sites` /
  `chartData` are present (done while diagnosing).
- End-to-end through the production parsers on the real downloaded pages
  (temporary in-crate test, removed afterwards): Düsseldorf home → **13
  stations**, Düsseldorf `/site/100005014` → **280 daily values**.

## Acceptance criteria

- The web tenants no longer 404: the station list and daily series parse from
  the plain HTML document.
- All existing scraping tests keep passing; new HTML-extraction tests are added.
- `make check`, `make test-rest` green; Docker-dependent `make test` /
  `make coverage` run when available.
- Docs (`scraping/README.md`, `eco_counter/README.md`, module docs) describe the
  HTML/flight transport instead of the removed `RSC: 1` mechanism.

## Definition of done

- [x] Plan file added
- [x] `RSC: 1` header removed from the page fetcher
- [x] HTML flight-payload extraction implemented and wired into `first_field`
- [x] Unit tests for HTML-wrapped / chunk-split / bare payloads
- [x] Docs updated
- [x] `make check` (fmt + clippy + prettier + audit) and `make test-rest` green
- [x] `cargo test scraping` green and live pages parse

## Addendum: duplicate station short codes (2026-10-08, live run)

Once the HTML transport was fixed, Düsseldorf (4732 measurements) and Köln
(8313) imported cleanly, but Hessen Mobil still failed **1.2 s in** with
`Database("db error")` — i.e. during station persistence, before any
measurement was read.

**Root cause.** The reworked Hessen payload lists **554 bicycle sites** and two
of them share the short code `1394`. The database enforces a unique
`(data_source_id, name)` index
([`V8`](../backend/migrations/V8__add_counting_station_and_channel_name_uniqueness.sql:42)),
so the second `counting_stations` insert raised a `unique_violation`, which the
repository surfaced as a generic DB error.

**Fix (provider-side).** [`parse_site_list`](../backend/src/adapter/driven/eco_counter/scraping/parsing.rs:241)
now calls `ensure_unique_names`: every station whose raw name collides is
renamed `"{name} ({external_id})"` (with a `#n` fallback if even that collides).
The external id is stable, so the rename is stable across runs and the core
still matches stations by external id; each station's single channel mirrors the
disambiguated name, keeping the channel `(counting_station_id, name)` index
satisfied too. This mirrors the repair [`V8`](../backend/migrations/V8__add_counting_station_and_channel_name_uniqueness.sql:11)
applied to already-imported duplicate rows.

The **persisted discovery index** is normalised on load as well
([`decode_index`](../backend/src/adapter/driven/eco_counter/scraping/adapter.rs:630)):
a failed earlier run may have cached the index with duplicate names, and the
cache window (seconds, default 300) would otherwise re-serve them without a
re-fetch.

**Verification.** The real Hessen home document parses to 554 stations with
**554 unique names**; dedicated unit tests pin the disambiguation (including the
collision fallback and persisted-index normalisation), and loopback HTTP tests
cover the full page-fetch path. End-to-end on the running stack: Hessen now
persists 554 stations (the two `1394` rows stored as `1394 (300066740)` /
`1394 (300071542)`), reports **RUNNING** instead of `Database("db error")`, and
imports measurements.
