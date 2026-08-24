import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    // Dev-only proxy so `fetch('/api/...')` reaches the backend during
    // `npm run dev`. In the Docker Compose stack the nginx container performs
    // the same reverse proxy, so the browser always talks same-origin.
    proxy: {
      '/api': 'http://localhost:8080',
    },
  },
})
