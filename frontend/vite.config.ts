import { readFileSync } from 'node:fs'
import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Read the version via `readFileSync` rather than a JSON import: importing
// `./package.json` without `with { type: 'json' }` is rejected by Vite's
// upcoming `configLoader: 'native'` (see GitHub issue #36).
const pkg = JSON.parse(readFileSync(path.resolve(import.meta.dirname, 'package.json'), 'utf8')) as {
  version: string
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // The frontend version is baked into the bundle at build time and read by
  // frontend/src/lib/version.ts; it is also injected for the Vitest unit suite
  // (vitest.config.ts) so both suites see the same value.
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  server: {
    // Dev-only proxy so `fetch('/api/...')` reaches the backend during
    // `npm run dev`. In the Docker Compose stack the nginx container performs
    // the same reverse proxy, so the browser always talks same-origin.
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
})
