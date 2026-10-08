import { useEffect, useState } from 'react'
import { getJson } from '../../lib/bff'

/// Fetches a JSON resource whenever `url` changes, exposing `loading` + `error`
/// state and resetting the previous data. Each stats card uses one instance of
/// this hook, so cards load and fail independently. The request goes through the
/// shared [`getJson`](../../lib/bff.ts), which validates every URL as a
/// root-relative, same-origin path before fetching, so a server-provided HATEOAS
/// link can never point the browser at another host (SSRF/CSRF).
export function useResource<T>(url: string | null) {
  const [data, setData] = useState<T | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!url) return
    let cancelled = false
    setLoading(true)
    setData(null)
    setError(false)
    getJson<T>(url)
      .then((json) => {
        if (!cancelled) setData(json)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [url])

  return { data, loading, error }
}
