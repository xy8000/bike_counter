import { useEffect, useState } from 'react'

/// Fetches a JSON resource whenever `url` changes, exposing `loading` + `error`
/// state and resetting the previous data. Each stats card uses one instance of
/// this hook, so cards load and fail independently.
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
    fetch(url)
      .then((response) => {
        if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
        return response.json() as Promise<T>
      })
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
