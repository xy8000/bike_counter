# 176 - BFF request URLs: strict same-origin allowlist (CSRF / SSRF findings)

Status: implemented

## Context

Two security findings were reported against the single JSON fetch helper
[`getJson`](frontend/src/lib/bff.ts:30) at **L17** (`fetch(...)`), both caused by
the same root cause: the request URL is assembled from user-controlled input and
was only checked with a handful of string predicates.

| # | Finding | Class | Severity | CWE | Effort | Reported |
|---|---|---|---|---|---|---|
| 1 | Client-Side Request Forgery (CSRF) via unsanitized user input | Security 4 / High, Vulnerability 3 / Minor | High | CWE-918 (server-side request forgery family) | 20 min | 30 days ago |
| 2 | Server-Side Request Forgery (SSRF) via unsanitized user input | Security 2 / Medium (Intentionality: Security), Vulnerability 2 / Major | Medium | CWE-918 | 30 min | 30 days ago |

### Root cause

`getJson(url)` passed `url` almost verbatim to `fetch` after the old
`assertSafeRequestUrl`, which only tested `startsWith('/')`,
`!startsWith('//')`, `!includes('\\')` and a control-character regex. That left
several ways for tainted input to influence the request destination and its
parameters:

1. **Tainted path segments.** [`fetchStationDetailPage`](frontend/src/features/stationDetail/api.ts:12),
   [`fetchStationOverview`](frontend/src/features/stationOverview/api.ts:8) and
   [`fetchDataSourceDetail`](frontend/src/features/dataSources/api.ts:12) interpolate
   the raw route parameter (`useParams()`, i.e. a fully attacker-controlled
   browser URL — see [`StationDetail`](frontend/src/features/stationDetail/StationDetail.tsx:128)
   and [`DataSourceDetail`](frontend/src/features/dataSources/DataSourceDetail.tsx:53))
   into `/api/bff/.../{id}` without encoding or charset validation. `?`, `#`,
   `/`, `%2f`, `%3f`, `%23` and dot segments therefore changed the requested
   route/query.
2. **No destination allowlist.** Any root-relative path was accepted, so a
   spoofed/compromised HATEOAS `_links.*.href` (a trust boundary) could point
   `getJson` at an unrelated same-origin endpoint (`/api/v1/...`, other BFF
   resources) — request forgery through a trusted-looking link.
3. **No query-parameter allowlist.** Tainted query fragments (`&url=…`,
   `&redirect=…`, duplicated keys) reached the backend unchanged.
4. **No fragment/`userinfo`/whitespace rejection**, no protection against
   parser rewriting (dot segments), and no post-fetch origin check, so `#…`,
   `@host` and `%2e%2e` survived validation and a redirect could still change
   the final destination.

## Goals

- Make the destination of every `getJson` request structurally impossible to
  leave the same-origin BFF namespace: parse with `URL`, reject everything that
  is not a literal root-relative path on the current origin, reject any input the
  parser had to rewrite, and match the pathname against an explicit **allowlist
  of the BFF routes** the backend actually serves.
- Validate every dynamic path segment against a strict charset
  (`[A-Za-z0-9._~-]+`), so a route parameter can never introduce a segment
  boundary, a query/fragment delimiter or a percent-encoded traversal.
- Allowlist the **query parameter names** to the set the BFF accepts
  (`min_lat`, `min_lng`, `max_lat`, `max_lng`, `as_of`, `exclude`,
  `exclude_new_stations`, `from`, `to`, `resolution`, `offset`, `limit`).
- Keep every legitimate request byte-identical (the normalization is a no-op for
  every URL the app builds today) and keep the existing error messages.

## Non-goals

- No change to the backend routes, DTOs or the nginx proxy.
- No change to the `fetch` call signature (single-argument `fetch(target)`), so
  the mocked-fetch assertions and the current 304/no-store caching behaviour stay
  untouched.
- No new dependency, no lint/Sonar suppression, no lowered gate thresholds.

## 1. `apiUrl.ts` - one parser, two guards

- `parseSameOriginUrl(url, message)` - the single place that interprets a URL:
  - rejects non-strings, empty strings, > 2048 chars;
  - rejects any C0 control character, DEL and whitespace (request splitting);
  - rejects a missing leading `/`, `//host` (protocol-relative), `\`
    (backslash smuggling) and `#` (fragment);
  - resolves against the current origin with `new URL(url, location.origin)` and
    rejects a different origin, a non-`http(s)` protocol, `user:pass@`
    credentials, a fragment, or an unparsable URL;
  - rejects any URL the parser had to rewrite
    (`url.split('?')[0] !== resolved.pathname`), i.e. dot segments and other
    "interpret me" input.
- `assertSafePathname(pathname, message)` - every segment of the normalized path
  must match `/^[A-Za-z0-9._~-]+$/` and must not be `.`/`..`. Because `%` is not
  in the charset, `%2e%2e`, `%2f`, `%00` and every other percent-encoded
  traversal/delimiter is rejected outright.
- `assertSafeQuery(search, message)` - rejects malformed percent escapes and
  percent-encoded control characters, then requires every key to be in the
  query-name allowlist.
- `assertSafeRequestUrl(url)` - same-origin structural guard (existing contract
  and error prefix `Refusing to fetch unsafe URL: …`), returning the normalized
  `pathname + search`.
- `assertSafeBffUrl(url)` - structural guard **plus** the `/api/bff/` prefix
  **plus** the route allowlist (error prefixes `Refusing to fetch non-BFF URL: …`
  and `Refusing to fetch non-allowlisted BFF URL: …`), returning the normalized
  request target.
- `safePathSegment(value)` - validates one id taken from the browser URL and
  returns it unchanged, throwing `Refusing to use unsafe path segment: …` for
  anything outside the safe segment charset (including `.`, `..`, `%2f`, `?`,
  `#`, `/`).
- `BFF_ROUTE_ALLOWLIST` - one `RegExp` per backend BFF route
  ([`bff_router`](backend/src/adapter/driving/rest/mod.rs:310)): stations,
  stations/sidebar(+stats), stations/search, stations/summary(+overview,
  graphs/{timeframe}, monthly), station-overview/{id}(+stats),
  station-detail/{id}(+overview, graphs/{timeframe}, monthly),
  data-sources(+{id}, {id}/imports), global-summary, version,
  assets/{id}/content. A new BFF route must be added here (fail-closed).

## 2. `bff.ts` - the sink stays, the destination is constrained

- `getJson` fetches `assertSafeBffUrl(url)`, i.e. the sink only ever receives an
  allowlisted, normalized, same-origin BFF path.
- After the response, a non-empty `response.url` must still be same-origin and
  parsable (`response.redirected` to another host is rejected), so an open
  redirect on the BFF cannot hand back a foreign body.
- Behaviour preserved: same `response.ok` check, same
  `` `${url} responded with ${status}` `` message (the caller's URL, not the
  normalized one) and same JSON parsing.

## 3. Call sites - no raw interpolation of route parameters

- [`stationDetail/api.ts`](frontend/src/features/stationDetail/api.ts:12):
  `/api/bff/station-detail/${safePathSegment(id)}`.
- [`stationOverview/api.ts`](frontend/src/features/stationOverview/api.ts:8):
  `/api/bff/station-overview/${safePathSegment(id)}`.
- [`dataSources/api.ts`](frontend/src/features/dataSources/api.ts:12):
  `safePathSegment(id)` in both the detail and the imports URL.
- [`stationsSummary/api.ts`](frontend/src/features/stationsSummary/api.ts:27):
  the `exclude` set is validated per id (`safePathSegment`) before it is joined
  with `,`, so a station id can never inject another query key. The URL is
  unchanged for valid ids.
- The thin HATEOAS-link wrappers (`fetchOverviewStats`, `fetchGraphs`,
  `fetchMonthlyTotals`, `fetchStationOverviewStats`, `fetchSidebarStats`,
  `fetchSummary*`) are now `async`, so a rejected URL surfaces as a rejected
  promise (what every caller's `.then/.catch` and `await` already expects)
  instead of a synchronous throw inside a render effect.

## 4. Tests

- New [`bff.test.ts`](frontend/src/lib/bff.test.ts): JSON happy path, unchanged
  query, non-ok status message, absolute/protocol-relative/traversal/
  non-allowlisted-BFF/outside-BFF/unknown-query rejection with **no** `fetch`
  call, cross-origin redirect response, same-origin final URL, `unwrapLinks`.
- [`apiUrl.test.ts`](frontend/src/lib/apiUrl.test.ts): allowlist coverage for
  every route shape, dot-segment rewriting, percent-encoded traversal, fragment,
  `userinfo`, whitespace, empty segments, unknown query keys, malformed and
  control-character escapes, and `safePathSegment` acceptance/rejection.
- Call-site regression tests: a tainted station/data-source id and a tainted
  `exclude` id are refused without a request
  ([`stationDetail/api.test.ts`](frontend/src/features/stationDetail/api.test.ts),
  [`stationOverview/api.test.ts`](frontend/src/features/stationOverview/api.test.ts),
  [`dataSources/api.test.ts`](frontend/src/features/dataSources/api.test.ts),
  [`stationsSummary/api.test.ts`](frontend/src/features/stationsSummary/api.test.ts)).
- The stations-summary fixtures that used the non-existent `/api/overview/…`,
  `/api/graphs/…`, `/api/monthly/…`, `/api/summary/…` pseudo-links now use the
  real BFF routes the backend serializes (see
  [`test-utils/stationsSummary.ts`](frontend/src/test-utils/stationsSummary.ts)),
  as do the detail-page fixtures.

## Definition of done

- [x] Plan file added
- [x] `apiUrl.ts` parser + segment/query/route allowlists + `safePathSegment`
- [x] `bff.ts` validates through `assertSafeBffUrl` and re-checks the response origin
- [x] Call sites no longer interpolate raw route parameters
- [x] Unit tests for the new guards and updated fixtures (615 tests green)
- [x] `make check` green (rustfmt, clippy, Prettier, `cargo audit`)
- [x] `npx tsc --noEmit` green
- [x] `npx vitest run --coverage` green (frontend `src` lines 96.49 %, above the 80 % gate)
- [x] `make test-playwright` not required: no rendered UI, route or backend change
