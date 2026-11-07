/// The frontend only ever talks to its own origin: every request URL — including
/// the HATEOAS `_links.*.href` values returned by the backend and every route
/// parameter taken from the browser URL — must be a root-relative,
/// same-origin path under the BFF namespace that matches one of the routes the
/// backend actually serves. Validating (and normalizing) that before every
/// `fetch` keeps a compromised response or any other user-controlled input from
/// turning the browser into a CSRF/SSRF gadget aimed at another host, another
/// API surface or an unintended BFF endpoint.
const BFF_PATH_PREFIX = '/api/bff/'

/// Upper bound on the accepted URL length. Every URL the app builds is far
/// shorter; longer input is rejected instead of being interpreted.
const MAX_URL_LENGTH = 2048

/// One dynamic path segment (a station id, a data-source id, a timeframe token).
/// Percent-encoding is deliberately not in the charset: encoded delimiters
/// (`%2f`, `%3f`, `%23`) and encoded dot segments (`%2e%2e`) can never reach the
/// request, and no leg the app builds needs them (ids are UUIDs/slugs).
const SAFE_SEGMENT = /^[A-Za-z0-9._~-]+$/

/// Source form of [`SAFE_SEGMENT`] for the route allowlist patterns.
const SEGMENT_SOURCE = '[A-Za-z0-9._~-]+'

/// C0 controls, DEL and whitespace: request splitting / header injection.
const CONTROL_OR_SPACE = /[\u0000-\u0020\u007f]/

/// A `%` that is not followed by exactly two hex digits (malformed escape).
const MALFORMED_ESCAPE = /%(?![0-9A-Fa-f]{2})/

/// A percent-encoded C0 control character or DEL.
const ENCODED_CONTROL = /%(?:0[0-9A-Fa-f]|1[0-9A-Fa-f]|7[Ff])/

/// The only query parameters the BFF accepts. Tainted input can therefore never
/// add or repurpose a request parameter (`?url=…`, `?redirect=…`, …).
const ALLOWED_QUERY_KEYS: ReadonlySet<string> = new Set([
  'min_lat',
  'min_lng',
  'max_lat',
  'max_lng',
  'as_of',
  'exclude',
  'exclude_new_stations',
  'from',
  'to',
  'resolution',
  'offset',
  'limit',
])

/// Allowlist of every route the backend BFF serves (see `bff_router` in
/// [`mod.rs`](backend/src/adapter/driving/rest/mod.rs:310)). Anything else is
/// refused even when it is same-origin, so a spoofed HATEOAS link cannot steer a
/// request to an unrelated endpoint. Fail-closed on purpose: a new BFF route
/// must be added here.
const BFF_ROUTE_ALLOWLIST: readonly RegExp[] = [
  /^\/api\/bff\/stations$/,
  /^\/api\/bff\/stations\/sidebar$/,
  /^\/api\/bff\/stations\/sidebar\/stats$/,
  /^\/api\/bff\/stations\/search$/,
  /^\/api\/bff\/stations\/summary$/,
  /^\/api\/bff\/stations\/summary\/overview$/,
  new RegExp(`^/api/bff/stations/summary/graphs/${SEGMENT_SOURCE}$`),
  /^\/api\/bff\/stations\/summary\/monthly$/,
  /^\/api\/bff\/global-summary$/,
  /^\/api\/bff\/version$/,
  new RegExp(`^/api/bff/station-overview/${SEGMENT_SOURCE}$`),
  new RegExp(`^/api/bff/station-overview/${SEGMENT_SOURCE}/stats$`),
  new RegExp(`^/api/bff/station-detail/${SEGMENT_SOURCE}$`),
  new RegExp(`^/api/bff/station-detail/${SEGMENT_SOURCE}/overview$`),
  new RegExp(`^/api/bff/station-detail/${SEGMENT_SOURCE}/graphs/${SEGMENT_SOURCE}$`),
  new RegExp(`^/api/bff/station-detail/${SEGMENT_SOURCE}/monthly$`),
  new RegExp(`^/api/bff/assets/${SEGMENT_SOURCE}/content$`),
  /^\/api\/bff\/data-sources$/,
  new RegExp(`^/api/bff/data-sources/${SEGMENT_SOURCE}$`),
  new RegExp(`^/api/bff/data-sources/${SEGMENT_SOURCE}/imports$`),
]

/// The origin every request must resolve against (the SPA's own origin).
function currentOrigin(): string {
  const origin = globalThis.location?.origin
  return typeof origin === 'string' && origin.length > 0 && origin !== 'null'
    ? origin
    : 'http://localhost'
}

/// Interprets `url` as a root-relative path on the current origin and returns
/// the parsed URL (which has already resolved any dot segments). Throws
/// `message` for anything that could escape the origin or the URL context: a
/// non-path form, a cross-origin absolute or protocol-relative URL,
/// backslash-smuggled paths, fragments, credentials, control characters and
/// whitespace.
function parseSameOriginUrl(url: string, message: string): URL {
  if (
    typeof url !== 'string' ||
    url.length === 0 ||
    url.length > MAX_URL_LENGTH ||
    CONTROL_OR_SPACE.test(url) ||
    url.includes('\\') ||
    url.includes('#') ||
    !url.startsWith('/') ||
    url.startsWith('//')
  ) {
    throw new Error(message)
  }
  const origin = currentOrigin()
  let resolved: URL
  try {
    resolved = new URL(url, origin)
  } catch {
    throw new Error(message)
  }
  if (
    resolved.origin !== new URL(origin).origin ||
    (resolved.protocol !== 'http:' && resolved.protocol !== 'https:') ||
    resolved.username !== '' ||
    resolved.password !== '' ||
    resolved.hash !== ''
  ) {
    throw new Error(message)
  }
  // Reject anything the parser had to rewrite (dot segments, percent-encoded
  // delimiters, …): callers only ever pass literal paths, so a mismatch means
  // the input was trying to be interpreted rather than read.
  if (url.split('?')[0] !== resolved.pathname) {
    throw new Error(message)
  }
  return resolved
}

/// Requires every segment of the normalized pathname to be a plain, safe
/// segment: no empty segment, no `.`/`..` and no percent-encoded byte.
function assertSafePathname(pathname: string, message: string): void {
  for (const segment of pathname.split('/').slice(1)) {
    if (segment === '.' || segment === '..' || !SAFE_SEGMENT.test(segment)) {
      throw new Error(message)
    }
  }
}

/// Requires every query key to be one the BFF accepts and rejects malformed or
/// control-character escapes.
function assertSafeQuery(search: string, message: string): void {
  if (search === '') return
  if (MALFORMED_ESCAPE.test(search) || ENCODED_CONTROL.test(search)) {
    throw new Error(message)
  }
  for (const key of new URLSearchParams(search).keys()) {
    if (!ALLOWED_QUERY_KEYS.has(key)) {
      throw new Error(message)
    }
  }
}

/// The normalized request target (`pathname` + `search`, no fragment).
function toRequestTarget(resolved: URL): string {
  return `${resolved.pathname}${resolved.search}`
}

/// Returns the normalized request target when `url` is a safe, root-relative,
/// same-origin path with safe segments and allowlisted query keys, and throws
/// otherwise.
export function assertSafeRequestUrl(url: string): string {
  const message = `Refusing to fetch unsafe URL: ${url}`
  const resolved = parseSameOriginUrl(url, message)
  assertSafePathname(resolved.pathname, message)
  assertSafeQuery(resolved.search, message)
  return toRequestTarget(resolved)
}

/// Returns the normalized request target when `url` is a safe path under the BFF
/// prefix **and** matches the [`BFF_ROUTE_ALLOWLIST`], and throws otherwise. This
/// is the guard every shared `getJson` request goes through.
export function assertSafeBffUrl(url: string): string {
  const message = `Refusing to fetch non-BFF URL: ${url}`
  const resolved = parseSameOriginUrl(url, message)
  if (!resolved.pathname.startsWith(BFF_PATH_PREFIX)) {
    throw new Error(message)
  }
  assertSafePathname(resolved.pathname, message)
  assertSafeQuery(resolved.search, message)
  if (!BFF_ROUTE_ALLOWLIST.some((route) => route.test(resolved.pathname))) {
    throw new Error(`Refusing to fetch non-allowlisted BFF URL: ${url}`)
  }
  return toRequestTarget(resolved)
}

/// Validates one dynamic path segment (an id taken from the browser URL) and
/// returns it unchanged. Ids are never percent-encoded by the callers, so any
/// value outside the safe segment charset — including `.`, `..`, `%2f`, `?`, `#`
/// and `/` — is refused instead of being interpolated into a request path.
export function safePathSegment(value: string): string {
  if (typeof value !== 'string' || value === '.' || value === '..' || !SAFE_SEGMENT.test(value)) {
    throw new Error(`Refusing to use unsafe path segment: ${String(value)}`)
  }
  return value
}
