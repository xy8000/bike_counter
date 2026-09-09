/// Locale-aware formatting helpers shared across the UI.
///
/// The single `LOCALE` constant is the central place to switch the UI language
/// later (e.g. `de-DE` → `en-GB`): every formatter below derives from it.
export const LOCALE = 'de-DE'

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(LOCALE).format(value)
}

export function formatTimestamp(iso: string | null): string {
  if (!iso) return 'never'
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return 'never'
  return date.toLocaleString(LOCALE, { dateStyle: 'short', timeStyle: 'short' })
}

/// Full date reference (`31.12.1998`) for chart tooltips.
export function formatFullDate(time: number): string {
  return new Date(time).toLocaleDateString(LOCALE, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  })
}

/// Full date + time reference (`31.12.1998, 14:00`) for chart tooltips.
export function formatFullDateTime(time: number): string {
  return new Date(time).toLocaleString(LOCALE, {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/// Escape HTML so untrusted backend strings can be embedded safely.
export function escapeHtml(value: string): string {
  const ampersand = String.fromCharCode(38)
  return value.replace(/[&<>"']/g, (char) => {
    const replacements: Record<string, string> = {
      '&': ampersand + 'amp;',
      '<': ampersand + 'lt;',
      '>': ampersand + 'gt;',
      '"': ampersand + 'quot;',
      "'": ampersand + '#39;',
    }
    return replacements[char] ?? char
  })
}
