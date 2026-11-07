/// Shared helpers for the BFF (backend-for-frontend) feature API modules: the
/// single JSON fetch wrapper and the HATEOAS `_links` unwrapper that every
/// feature used to re-declare.

import { assertSafeBffUrl } from './apiUrl'

/// The wire shape of one HATEOAS link entry: the backend serializes `_links`
/// values as `LinkDto` (`{ href, templated }`).
export type RawLink = { href: string; templated?: boolean }

/// Rejects a response whose final URL (`response.url`, i.e. after redirects) is
/// not on the app's own origin, so an open redirect on the BFF cannot hand a
/// foreign body to the caller. Mocked/opaque responses without a URL are left to
/// the regular `ok` check.
function assertSameOriginResponse(url: string, response: Response): void {
  if (!response.url) return
  const origin = globalThis.location?.origin
  if (!origin) return
  let finalOrigin: string
  try {
    finalOrigin = new URL(response.url).origin
  } catch {
    throw new Error(`Refusing to read an unparsable response URL: ${url}`)
  }
  if (finalOrigin !== new URL(origin).origin) {
    throw new Error(`Refusing to read a cross-origin response: ${url}`)
  }
}

/// Fetch `url` and parse the JSON body, throwing on a non-ok response. Every
/// request URL — including the HATEOAS `_links.*.href` values the feature
/// modules pass through and the ids interpolated into BFF paths — is validated
/// against the same-origin BFF route allowlist (plus the segment/query
/// allowlists) before the fetch, and the normalized target is what reaches
/// `fetch`. A compromised or spoofed response, a crafted deep link or any other
/// user-controlled input therefore cannot point the browser at another host, at
/// another API surface or at an unintended BFF endpoint (CSRF/SSRF).
export async function getJson<T>(url: string): Promise<T> {
  const target = assertSafeBffUrl(url)
  const response = await fetch(target)
  assertSameOriginResponse(url, response)
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
