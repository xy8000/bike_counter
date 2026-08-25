import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
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
