/// Shared helpers for the BFF (backend-for-frontend) feature API modules: the
/// single JSON fetch wrapper and the HATEOAS `_links` unwrapper that every
/// feature used to re-declare.

/// The wire shape of one HATEOAS link entry: the backend serializes `_links`
/// values as `LinkDto` (`{ href, templated }`).
export type RawLink = { href: string; templated?: boolean }

/// Fetch `url` and parse the JSON body, throwing on a non-ok response. The
/// caller is responsible for passing a safe, same-origin URL.
export async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} responded with ${response.status}`)
  return response.json() as Promise<T>
}

/// Reduce a `{ key: LinkDto }` map to `{ key: href }`, so a page shell only has
/// to carry the href strings its cards need.
export function unwrapLinks<T extends object>(links: Record<string, RawLink>): T {
  const out: Record<string, string> = {}
  for (const [key, value] of Object.entries(links)) {
    out[key] = value.href
  }
  return out as unknown as T
}
