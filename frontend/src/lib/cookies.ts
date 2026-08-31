/// Minimal `document.cookie` helpers used by the view-settings persistence.
/// Cookies survive a bare URL revisit (the URL stays the source of truth for
/// sharing; the cookie is the fallback that re-populates it).

export function getCookie(name: string): string | null {
  if (typeof document === 'undefined') return null
  const prefix = `${name}=`
  for (const part of document.cookie.split(';')) {
    const trimmed = part.trim()
    if (trimmed.startsWith(prefix)) {
      return decodeURIComponent(trimmed.slice(prefix.length))
    }
  }
  return null
}

export function setCookie(name: string, value: string, maxAgeSeconds: number): void {
  if (typeof document === 'undefined') return
  document.cookie = `${name}=${encodeURIComponent(value)}; Path=/; Max-Age=${maxAgeSeconds}; SameSite=Lax`
}
