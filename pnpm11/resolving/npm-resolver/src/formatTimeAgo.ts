export function formatTimeAgo (date: Date): string | null {
  const diffMs = Date.now() - date.getTime()
  // An invalid date, or a future one from clock skew.
  if (isNaN(diffMs) || diffMs < 0) {
    return null
  }

  const diffSec = Math.floor(diffMs / 1000)
  const diffMin = Math.floor(diffSec / 60)
  const diffHour = Math.floor(diffMin / 60)
  const diffDay = Math.floor(diffHour / 24)
  const elapsedByUnit = [
    { unit: 'year', count: Math.floor(diffDay / 365) },
    { unit: 'month', count: Math.floor(diffDay / 30) },
    { unit: 'day', count: diffDay },
    { unit: 'hour', count: diffHour },
    { unit: 'minute', count: diffMin },
  ]
  const largest = elapsedByUnit.find(({ count }) => count > 0)
  if (largest == null) return 'a few seconds ago'
  return `${largest.count} ${largest.unit}${largest.count === 1 ? '' : 's'} ago`
}
