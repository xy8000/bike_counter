import { defineConfig } from '@playwright/test'

/// End-to-end browser tests for the React frontend.
///
/// The tests run against the production frontend served by nginx on port 8081
/// (which reverse-proxies /api to the backend BFF). The stack is brought up and
/// torn down by `scripts/e2e-playwright.sh` (make test-playwright); this config
/// does not manage a web server itself. Override the target with FRONTEND_URL.
export default defineConfig({
  testDir: './e2e',
  // A single worker keeps the run deterministic against the shared, still
  // importing Docker stack.
  fullyParallel: false,
  workers: 1,
  // Upper bounds so the suite can never wait forever on a hung step: generous,
  // not tight, so healthy runs are unaffected.
  timeout: 90_000,
  globalTimeout: 20 * 60 * 1000,
  expect: {
    timeout: 20_000,
  },
  retries: process.env.CI ? 2 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: process.env.FRONTEND_URL ?? 'http://localhost:8081',
    viewport: { width: 1280, height: 800 },
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    actionTimeout: 30_000,
    navigationTimeout: 30_000,
  },
  projects: [{ name: 'chromium' }],
})
