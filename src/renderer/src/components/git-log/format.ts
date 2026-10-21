export function formatRelativeDate(ts: number | undefined, now = Date.now()): string {
  if (!ts) return ''
  const diff = Math.max(0, now - ts)
  const min = Math.floor(diff / 60_000)
  if (min < 1) return 'just now'
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.floor(hr / 24)
  if (day < 30) return `${day}d ago`
  return new Date(ts).toISOString().slice(0, 10)
}

export function shortHash(oid: string): string {
  return oid.slice(0, 7)
}

export function formatAbsoluteDate(ts: number | undefined): string {
  if (!ts) return ''
  return new Date(ts).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' })
}
