# 122 - Frontend unit coverage to 80% (whole src, without Playwright)

Status: implemented

## Goal

Raise the **frontend unit-test (Vitest) line coverage of the whole
`frontend/src`** to **≥ 80 %**, measured **without** the Playwright browser e2e
suite (which remains an additional layer on top). Concretely:

- The Vitest suite currently runs in the `node` environment and its coverage is
  scoped to six pure-logic modules
  ([`frontend/vitest.config.ts`](../frontend/vitest.config.ts:31)). Measured
  against the whole `src` (~8000 lines, 58 `.tsx` components + hooks/API
  modules) coverage is ~10 %.
- Target: the full `src` (all `.ts`/`.tsx` production files) reaches **≥ 80 %
  line coverage** from the Vitest suite alone, enforced by a coverage threshold
  in the Vitest config so `make test-unit-coverage` fails below it (mirroring
  the backend `make coverage` gate philosophy).
- Playwright e2e keeps running as today ([agents.md](../agents.md:98)) — it is
  *in addition* and does not contribute to this 80 % number.

## Current state

- [`frontend/package.json`](../frontend/package.json:15) — `test:unit`,
  `test:unit:coverage` scripts; only `vitest` + `@vitest/coverage-v8` dev deps.
- [`frontend/vitest.config.ts`](../frontend/vitest.config.ts:19) — `node`
  environment, `include: ['src/**/*.test.{ts,tsx}']`, coverage `include` scoped
  to 6 files, no thresholds.
- Existing tests: `lib/format`, `lib/geo`, `lib/utils`,
  `features/map/clusterStations`, `features/stationDetail/resolution`,
  `features/stationDetail/timeframes` (50 tests).
- React components are exercised only by Playwright
  ([agents.md](../agents.md:93)); nothing renders them in a DOM unit test today.

## Approach

The bulk of `src` is React components, so reaching 80 % of the whole tree
requires a DOM test environment plus a component-testing library:

1. **Test tooling** — add `jsdom`, `@testing-library/react`,
   `@testing-library/jest-dom`, `@testing-library/user-event` (dev deps), and a
   `vitest.setup.ts` that imports `@testing-library/jest-dom`.
2. **Vitest config** — switch `environment` to `jsdom`, add `setupFiles`,
   broaden the coverage `include` to the whole `src` (excluding `*.test.*`),
   keep `text` + `lcov` reporters, and add coverage `thresholds` enforcing
   `lines: 80` (plus a sensible `statements`/`functions`/`branches` guard).
3. **Heavy-dependency mocks** — MapLibre/react-maplibre/pmtiles/recharts are
   hard to instantiate under jsdom; provide module-level mocks (per-test-file
   `vi.mock` or a shared mock module) so feature components render without a
   real map/chart. `@vis.gl/react-maplibre` and `recharts` components become
   lightweight stand-ins that still exercise the surrounding logic.
4. **Tests first, in feature groups** (new files, colocated `*.test.ts(x)`),
   each group bringing measurable lines:
   - `lib/` — `cookies`, `ErrorBoundary`, `map` (marker image + flag picker),
     `geo`/`format`/`utils` already covered, extend branches.
   - `settings/` — `TrendSettingsContext`, `useTimeframeSettings`,
     `TimeframeSettingsLabel`, `SettingsDialog`, `TrendSettingsIllustration`.
   - `map/` — `BaseMap`, `MapView`, `MapPage` (mock maplibre/react-maplibre).
   - `stations/` + `search/` — `api`, `useStationSearch`, `useVisibleStations`,
     `StationListItem`, `SearchDialog`.
   - `sidebar/` — `Sidebar`, `SidebarListItem`, `LeftPanel`, `SidebarHandle`.
   - `header/` — `api`, `TopBar`, `SearchableHeader`, `GlobalSummaryDialog`,
     `useGlobalSummary`.
   - `stationOverview/` — `api`, `MetricCard`, `TrendIcon`, `TotalBikesCard`,
     `StationOverview`, `useStationOverview`, `Skeletons`.
   - `stationDetail/` — `api`, `chartUtils`, hooks (`useResource`,
     `useStationDetailPage`, `useStationGraphs`, `useStationMonthly`,
     `useStationOverviewStats`), chart components (`MonthlyBarChart`,
     `TimeSeriesBarChart`, `HourRadar`, `WeekdayRadar`, `ChannelPie`,
     `SharePie`, `ChartCard`, `ChartEmptyState`, `ChartLimitNotice`,
     `KeyFacts`, `Skeletons`, `DetailMap`, `StationDetail`).
   - `stationsSummary/` — `api`, summary hooks, `SummaryMap`, `StationsSummary`.
   - `dataSources/` — `api`, `ImportStatus`, `DataSourceMap`, `DataSourceDetail`,
     `DataSourcesList`, `DataSourcesList`/detail pieces.
   - `components/ui/*` — render-smoke tests for the shadcn wrappers
     (button/badge/card/checkbox/dialog/input/label/select/switch/separator/
     skeleton/scroll-area/tooltip/chart primitives).
5. **Iterate on the report** — run `npm run test:unit:coverage` after each
   group, add the missing exercises, until the whole-`src` line coverage is
   ≥ 80 %.
6. **Gate/docs** — document the new threshold in
   [`frontend/vitest.config.ts`](../frontend/vitest.config.ts),
   [`agents.md`](../agents.md:90) (frontend flag now gated at 80 % lines, DOM
   component tests included; Playwright stays additional) and
   [`CONTRIBUTING.md`](../CONTRIBUTING.md); update this plan's checkboxes.
   Keep Codecov informational or make the frontend project status
   non-informational only after confirming the CI number (see Notes).

## Decisions

- **Line coverage is the gating metric** (`lines: 80`), matching how the
  backend gate and Codecov reports are phrased. Branch/function thresholds are
  set where the suite lands without inflating effort.
- **Components get DOM unit tests**; the `environment` moves to `jsdom` for all
  tests (pure-logic tests run fine under jsdom too). No separate project split
  is needed to keep the report simple.
- **`main.tsx`/`vite-env.d.ts`** — `main.tsx` only calls
  `createRoot(...).render`, which needs a real mount target; it is exercised by
  Playwright. It stays out of the unit coverage `include` (entry-point
  scaffolding, no logic), matching how the backend excludes `#[cfg(test)]` /
  entry scaffolding from production coverage. All real feature modules are in.
- **Playwright is not counted** toward the 80 %; it stays as the additional
  browser layer documented in agents.md.

## Risks

- Big surface: ~58 components + ~35 logic/hook modules. Mitigated by grouping
  tests per feature and iterating on the coverage report.
- MapLibre/react-maplibre/recharts need faithful lightweight mocks; the mocked
  components must still forward props/data so downstream code paths run.
- jsdom lacks canvas/layout used by Recharts; charts are asserted at the
  "rendered without crashing + data prepared" level rather than pixel output.
- Radix UI (`dialog`, `select`, `tooltip`, `scroll-area`) needs
  `ResizeObserver`/`matchMedia`/`PointerEvent` shims in the setup file.

## Definition of done

- [x] Plan reviewed by the architect
- [x] jsdom + Testing Library tooling installed; `vitest.setup.tsx` with jest-dom + jsdom shims (maplibre/react-maplibre/recharts module mocks; matchMedia/ResizeObserver/PointerEvent shims)
- [x] `vitest.config.ts`: jsdom environment, whole-`src` coverage include (main.tsx/vite-env/types excluded), ≥ 80 % lines/statements threshold (+ 75 % functions, 70 % branches); `src/jest-dom.d.ts` registers the matcher types for `tsc`
- [x] Tests cover `lib/`, `settings/`, `map/`, `stations/`, `search/`, `sidebar/`, `header/`, `stationOverview/`, `stationDetail/`, `stationsSummary/`, `dataSources/` and `components/ui/*`
- [x] `npm run test:unit:coverage` reports whole-`src` coverage ≥ 80 % and the threshold gate passes (84 files / 540 tests; lines 94.91 %, statements 94.79 %, functions 94.11 %, branches 87.5 %)
- [x] `make check` green (fmt/clippy/audit + prettier), `make frontend-build` green (tsc + vite)
- [x] Playwright e2e unchanged (no UI behaviour changed)
- [x] `agents.md` + `CONTRIBUTING.md` updated; dual coverage gate in `Makefile` (`make coverage` = backend + frontend) and mandated per-flag Codecov status in `codecov.yml`; plan `Status:` + checkboxes current
