/** Parses a stored timestamp; values saved without a timezone are UTC. */
export function parseStamp(stamp: string): Date {
  return new Date(/(Z|[+-]\d\d:?\d\d)$/.test(stamp) ? stamp : `${stamp}Z`)
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
]

const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })

/** "just now", "5 minutes ago", "yesterday", "3 weeks ago". */
export function relativeTime(stamp: string | null | undefined): string {
  if (!stamp) return 'never'
  const seconds = Math.round((parseStamp(stamp).getTime() - Date.now()) / 1000)
  if (Number.isNaN(seconds)) return ''
  if (Math.abs(seconds) < 45) return 'just now'
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit)
  }
  return formatter.format(Math.round(seconds / 60), 'minute')
}
