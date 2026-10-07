/// The frontend only ever talks to its own origin: every request URL — including
/// the HATEOAS `_links.*.href` values returned by the backend — must be a
/// root-relative, same-origin path. Validating that before every `fetch` keeps a
/// compromised or spoofed response from turning the browser into an SSRF/CSRF
/// gadget aimed at an arbitrary (cross-origin) URL.
const BFF_PATH_PREFIX = '/api/bff/'

/// Returns `url` when it is a safe, root-relative, same-origin path and throws
/// otherwise. Rejects absolute URLs (`https://…`), protocol-relative URLs
/// (`//host`), backslash-smuggled paths (`/\host`) and any control characters
/// that could break out of the URL context.
export function assertSafeRequestUrl(url: string): string {
  const safe =
    url.startsWith('/') &&
    !url.startsWith('//') &&
    !url.includes('\\') &&
    !/[\u0000-\u001f\u007f]/.test(url)
  if (!safe) {
    throw new Error(`Refusing to fetch unsafe URL: ${url}`)
  }
  return url
}

/// Returns `url` when it is a safe, same-origin path under the BFF prefix and
/// throws otherwise. The stricter allowlist for the stations API, whose request
/// URL can come straight from a server-provided link (`_links.stats.href`).
export function assertSafeBffUrl(url: string): string {
  const safe =
    url.startsWith(BFF_PATH_PREFIX) && !url.includes('\\') && !/[\u0000-\u001f\u007f]/.test(url)
  if (!safe) {
    throw new Error(`Refusing to fetch non-BFF URL: ${url}`)
  }
  return url
}
