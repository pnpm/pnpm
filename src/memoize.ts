export type CachedPromises<T> = {
  [name: string]: Promise<T>
}

/**
 * Save promises for later
 */
export default function memoize <T>(locks: CachedPromises<T>, key: string, fn: () => Promise<T>) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
