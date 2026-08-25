import type { GlobalSummary } from './types'

/// Load the whole-system summary for the header.
export async function fetchGlobalSummary(): Promise<GlobalSummary> {
  const response = await fetch('/api/bff/global-summary')
  if (!response.ok) throw new Error(`global summary responded with ${response.status}`)
  return response.json() as Promise<GlobalSummary>
}
