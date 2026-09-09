import path from 'node:path'
import { defineConfig } from 'vitest/config'

/// Vitest unit-test configuration for the frontend.
///
/// The suite runs in the `jsdom` environment: pure-logic modules (formatting,
/// geo/bounds math, clustering, timeframe/resolution helpers) run there as
/// cheaply as in `node`, and React components can be rendered with
/// @testing-library/react. This is the suite that drives the frontend coverage
/// gate to ≥ 80 % lines over the whole `src` — see agents.md. The Playwright
/// browser e2e suite (frontend/e2e) is an additional layer and is not part of
/// this coverage number.
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
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./vitest.setup.tsx'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      reportsDirectory: 'coverage',
      // Cover the whole application source. Test files and the mount entry
      // point (main.tsx: it only calls createRoot().render and is exercised by
      // Playwright; there is no component logic to assert on) are excluded, as
      // is the ambient type declaration.
      include: ['src/**/*.{ts,tsx}'],
      exclude: ['src/**/*.test.{ts,tsx}', 'src/main.tsx', 'src/vite-env.d.ts', 'src/**/types.ts'],
      // The unit suite alone (no Playwright) must keep whole-src line coverage
      // at or above 80 % — mirroring the backend `make coverage` gate.
      thresholds: {
        lines: 80,
        statements: 80,
        functions: 75,
        branches: 70,
      },
    },
  },
})
