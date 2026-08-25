/// Locale-aware formatting helpers shared across the UI.

export function formatNumber(value: number): string {
  return new Intl.NumberFormat('de-DE').format(value)
}

export function formatTimestamp(iso: string | null): string {
  if (!iso) return 'never'
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return 'never'
  return date.toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' })
}

/// Escape HTML so untrusted backend strings can be embedded safely.
export function escapeHtml(value: string): string {
  const ampersand = String.fromCharCode(38)
  return value.replace(/[&<>"']/g, (char) => {
    const replacements: Record<string, string> = {
      '&': ampersand + 'amp;',
      '<': String.fromCharCode(60) + 'lt;',
      '>': String.fromCharCode(62) + 'gt;',
      '"': String.fromCharCode(34) + 'quot;',
      "'": String.fromCharCode(39) + '#39;',
    }
    return replacements[char] ?? char
  })
}
