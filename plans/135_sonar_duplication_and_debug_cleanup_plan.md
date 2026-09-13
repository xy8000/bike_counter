# 135 - Sonar duplication cleanup + debug artifact removal

Status: implemented

## Problem

Sonar's "New code" duplication report flags duplicated blocks introduced by the
recent frontend refactors, plus two debug HTML pages that are near-copies of each
other:

| File | Duplicated % | Lines |
|---|---|---|
| `frontend/public/pmtiles-debug.html` | 61.0% | 25 |
| `frontend/public/pmtiles-debug-v6.html` | 50.0% | 25 |
| `frontend/src/features/stationDetail/StationDetail.tsx` | 29.7% | 144 |
| `frontend/src/features/stationDetail/api.ts` | 27.9% | 17 |
| `frontend/src/features/stationsSummary/StationsSummary.tsx` | 26.2% | 143 |
| `frontend/src/features/stationsSummary/useStationsSummaryPage.test.tsx` | 22.4% | 22 |
| `frontend/src/features/stationsSummary/api.ts` | 20.5% | 17 |
| `frontend/src/features/stations/useStationSearch.test.tsx` | 18.5% | 29 |
| `frontend/e2e/dark-mode.spec.ts` | 14.2% | 15 |
| `frontend/src/features/stationsSummary/api.test.ts` | 13.5% | 22 |
| `frontend/src/features/dataSources/DataSourceDetail.tsx` | 9.3% | 22 |
| `frontend/src/features/dataSources/DataSourcesList.tsx` | 8.4% | 22 |
| `frontend/e2e/map.spec.ts` | 7.5% | 16 |

## Approach

### 1. Remove the debug artifacts (unused)
`frontend/public/pmtiles-debug.html`, `frontend/public/pmtiles-debug-v6.html` and
the vendored `frontend/public/debug-libs/` are debug-only: the production app
bundles `maplibre-gl`/`pmtiles` from npm, and nothing (app, tests, Makefile,
docs) references the debug pages. Deleting them removes the two HTML duplication
findings and the vendored bundle entirely. The now-dead
`frontend/public/debug-libs/**` exclusion is dropped from
[`sonar-project.properties`](../sonar-project.properties:4).

### 2. Shared BFF request helpers
New `frontend/src/lib/bff.ts` exports `RawLink`, `getJson` and `unwrapLinks`; the
five feature `api.ts` modules stop re-declaring them.
[`stations/api.ts`](../frontend/src/features/stations/api.ts:8) keeps its
`assertSafeBffUrl` guard by calling the shared `getJson` with the sanitized URL.

### 3. Shared chart-section helpers
New `frontend/src/features/stationDetail/sections.tsx` holds the helpers both the
detail and summary pages duplicated: `timeframeConfig`, `graphsLink`,
`aggregateWeekdayRadar`, `aggregateHourRadar` and the section bodies
(`overviewBody`, `statisticsBody`, `perSeriesStatsBody`, `monthlyBody`).

### 4. Shared station-page shell
New `frontend/src/features/stations/useStationActions.ts` (`openDetail` +
`findOnMap`, duplicated verbatim in four pages) and
`frontend/src/components/StationPage.tsx` (the shared `<div><SearchableHeader>
<main><div>` scaffolding), used by StationDetail, StationsSummary,
DataSourceDetail and DataSourcesList.

### 5. Test helper extraction
The `ok(data)` fetch-response stub was re-declared in 32 unit-test files; it now
lives once in `frontend/src/test-utils/http.ts` and every suite imports it. The
summary shell fixture (`rawSummaryPage`, bounds) is shared via
`frontend/src/test-utils/stationsSummary.ts` by
[`api.test.ts`](../frontend/src/features/stationsSummary/api.test.ts) and
[`useStationsSummaryPage.test.tsx`](../frontend/src/features/stationsSummary/useStationsSummaryPage.test.tsx).
[`useStationSearch.test.tsx`](../frontend/src/features/stations/useStationSearch.test.tsx)
gained a `renderLoadedSearch` helper for its six identical stub+render+wait
openers. `src/test-utils/**` is excluded from coverage (no application logic).

### 6. E2E helper extraction
[`e2e/helpers.ts`](../frontend/e2e/helpers.ts) gained `openMap(page, url)`
(the `goto` + `waitForStations` pair repeated ~25×), `firstMarker(page)`,
`readTheme(page, selector)` and `expectTheme(page, scheme, selector)`. The
`goto`/`waitForStations` pairs across `detail`, `flags`, `search`, `settings`,
`sidebar`, `summary`, `url` and `map` specs now call `openMap`; `map.spec.ts` and
`dark-mode.spec.ts` were rewritten on top of the new helpers.

### 7. New tests
`frontend/src/features/stations/useStationActions.test.tsx` covers the extracted
navigation hook (detail route + find-on-map with and without coordinates), which
otherwise dropped below the coverage bar when it was extracted.

## Definition of done

- [x] Plan file added
- [x] Debug pages + `debug-libs/` removed, references/exclusion cleaned
- [x] Shared `lib/bff.ts` adopted by the feature `api.ts` modules
- [x] Shared chart-section helpers adopted by detail + summary
- [x] Shared station-page shell/actions adopted by the four pages
- [x] Test + e2e duplication reduced
- [x] `make check`, frontend unit tests, `make coverage` green
- [x] `make test-playwright` green
- [x] Plan `Status:` updated
