import { useEffect, useState } from 'react'

/// The self-hosted basemap archive served by nginx (the same file the `pmtiles`
/// protocol reads via HTTP range requests). Exported so tests target it.
export const TILES_URL = '/tiles/map.pmtiles'
/// How often the readiness probe retries while the archive is still being built
/// in the background.
const POLL_MS = 2000

/// Whether the basemap archive (`/tiles/map.pmtiles`) is served yet. On a fresh
/// deployment the backend builds it in the background after startup (see plan
/// 123 — startup no longer blocks on it), so this polls with a `HEAD` request
/// until it returns `200` and the map can render. A network error (archive or
/// nginx still coming up) is treated as "not ready" and retried.
export function useTilesReady(): boolean {
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined

    async function poll() {
      try {
        const response = await fetch(TILES_URL, { method: 'HEAD', cache: 'no-store' })
        if (cancelled) return
        if (response.ok) {
          setReady(true)
          return
        }
      } catch {
        // Network error while the archive/nginx is coming up: keep retrying.
        if (cancelled) return
      }
      timer = setTimeout(poll, POLL_MS)
    }

    void poll()

    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [])

  return ready
}
