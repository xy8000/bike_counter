# 134 - Fix remaining Sonar maintainability findings (hand-written code)

Status: implemented

## Scope

Fix the open SonarCloud maintainability reliability/adaptability findings in
**hand-written** sources only. The vendored, generated bundle
`frontend/public/debug-libs/pmtiles.js` is intentionally left to the existing
`sonar.exclusions` in [`sonar-project.properties`](../sonar-project.properties:4)
(plan 129), per the maintainer's decision: it has no in-repo generator, relies on
`var` hoisting + repeated `var i/s/c/...` declarations, and its biggest finding is
a 126-complexity fflate decompression hot loop that is unsafe to hand-rewrite.

Targets: `frontend/src`, `frontend/e2e`, [`frontend/vitest.setup.tsx`](../frontend/vitest.setup.tsx),
[`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh) and
[`backend/Dockerfile`](../backend/Dockerfile).

## What changed

### A. Small/mechanical lints
- [`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh:184) — positional parameter assigned to a `local query`.
- [`backend/Dockerfile`](../backend/Dockerfile:11) — `apk add` packages sorted alphanumerically.
- [`frontend/e2e/helpers.ts`](../frontend/e2e/helpers.ts:52) — `getAttribute('data-count')` → `.dataset.count`.
- [`frontend/e2e/detail.spec.ts`](../frontend/e2e/detail.spec.ts:206) and
  [`frontend/e2e/sidebar.spec.ts`](../frontend/e2e/sidebar.spec.ts:54) — fixed waits replaced by `expect.poll` with `intervals` (URL-stabilisation / zoom-throttle).
- [`frontend/src/features/settings/useTimeframeSettings.ts`](../frontend/src/features/settings/useTimeframeSettings.ts:18) — `VALID_TIMEFRAMES` array → `Set` + `.has()`.
- [`frontend/src/components/ui/chart.tsx`](../frontend/src/components/ui/chart.tsx:47) and
  [`frontend/src/features/settings/TrendSettingsContext.tsx`](../frontend/src/features/settings/TrendSettingsContext.tsx:38) — context value wrapped in `useMemo`.
- [`frontend/src/features/map/BaseMap.tsx`](../frontend/src/features/map/BaseMap.tsx:116) and
  [`frontend/src/features/map/MapView.tsx`](../frontend/src/features/map/MapView.tsx:73) — optional chaining.
- [`frontend/src/components/ui/chart.test.tsx`](../frontend/src/components/ui/chart.test.tsx:156) — `toHaveLength` (×4).
- [`frontend/vitest.setup.tsx`](../frontend/vitest.setup.tsx:37) — non-empty `observe`/`unobserve`/`disconnect`, non-empty module-scope `FakeMaplibreGlMap` (no more inline `FakeMap`).

### B. React "mark props as read-only" (S6759)
Wrapped the props type in `Readonly<…>` for every flagged component: `MapView`,
`StationDetail`, `DataSourceDetail` (×2), `DataSourceMap`, `DataSourcesList` (×2),
`ImportStatus` (×3), `GlobalSummaryDialog`, `SearchableHeader`, `TopBar`,
`BaseMap`, `SearchDialog`, `SettingsDialog`, `TimeframeSettingsLabel`,
`TrendSettingsContext`, `TrendSettingsIllustration`, `LeftPanel`, `Sidebar` (×2),
`SidebarHandle`, `SidebarListItem`, `ChannelPie`, `ChartCard`, `ChartEmptyState`,
`ChartLimitNotice`, `DetailMap`, `HourRadar`, `KeyFacts`, `MonthlyBarChart`,
`SharePie`, `TimeSeriesBarChart`, `WeekdayRadar`, `MetricCard`,
`StationOverview`, `TotalBikesCard`, `TrendIcon`, `StationListItem`.

### C. Nested ternaries extracted
- `MapView` — `markerState()` helper + `StationPopupBody` subcomponent.
- `StationDetail` — `timeframeConfig()`, `graphLinkFor()`, and section body helpers (`overviewBody`, `statisticsBody`, `perChannelStatsBody`, `monthlyBody`).
- `StationsSummary` — `timeframeConfig()`, `summaryGraphLink()`, and the same section body helpers.
- `MonthlyBarChart` — `trendFromDelta()`, `trendTextClass()`, `deltaLabel()`.
- `SharePie` — `if/else` body construction.
- `timeframes.ts` — `customTimeframeConfig` axis selection written as an `if/else` chain.
- `MetricCard`, `StationOverview` — `deltaLabel()` and `OverviewStatsBlock`.
- `DataSourceDetail` — `formatImportDuration()`.

### D. Cognitive complexity / nesting
- `MapView` (`MapView`) — popup body + marker state extracted (19 → below the 15 budget).
- `StationDetail` (`DetailContent`) — six ternaries + section bodies extracted (26 → below 15).
- `StationsSummary` (`SummaryContent`) — same extraction (27 → below 15).
- `resolution.ts` (`resolutionOptions`) — individual range split into `individualOptions`.
- `useVisibleStations.ts` — the effect body rewritten with `async`/`await` instead of nested `.then`.

No functional/UI change intended; DOM output is unchanged.

## Verification

- `npx prettier --check .` — clean.
- `npx tsc --noEmit` — clean.
- `npm run test:unit:coverage` — green (all Vitest files pass; whole-`src` coverage thresholds met).
- `./scripts/fmt-test.sh` — `fmt-test: OK` (cargo fmt + clippy `-D warnings`).
- `make test-playwright` — **74 passed** (3.0 min), `e2e-playwright: OK`.

## Definition of done

- [x] Plan file added
- [x] Group A (mechanical lints) fixed
- [x] Group B (read-only props) fixed
- [x] Group C (nested ternaries) fixed
- [x] Group D (complexity/nesting) fixed
- [x] `make check` (fmt/clippy/prettier) green
- [x] `npm run test:unit` green
- [x] `make coverage` green (frontend ≥ 80 %)
- [x] `make test-playwright` green
- [x] Plan `Status:` and boxes updated
