import path from 'node:path'
import { defineConfig } from 'vitest/config'

/// Vitest unit-test configuration for the frontend.
///
/// The suite runs in the Node environment because it targets the pure-logic
/// modules (formatting, geo/bounds math, clustering, timeframe/resolution
/// helpers) — it does not need a DOM. React components are covered by the
/// Playwright e2e suite (frontend/e2e), not by this unit lcov report.
///
/// Coverage uses the v8 provider and emits text + lcov (the lcov file is what
/// .github/workflows/ci.yml uploads to Codecov under the `frontend` flag).
export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      reportsDirectory: 'coverage',
      // Scope the report to the pure-logic modules the unit suite exercises.
      // The v8 provider always enumerates every `include`d file, so the rest of
      // src (React components, covered by Playwright e2e rather than this unit
      // lcov) is left out instead of being reported at ~0 %. Extend this list
      // as the unit suite grows.
      include: [
        'src/lib/format.ts',
        'src/lib/geo.ts',
        'src/lib/utils.ts',
        'src/features/map/clusterStations.ts',
        'src/features/stationDetail/resolution.ts',
        'src/features/stationDetail/timeframes.ts',
      ],
    },
  },
})
