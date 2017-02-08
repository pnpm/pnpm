import pLimit = require('p-limit')

type CachedPromises<T> = {
  [name: string]: Promise<T>
}

export type MemoizedFunc<T> = (key: string, fn: () => Promise<T>) => Promise<T>

/**
 * Save promises for later
 */
export default function memoize <T>(concurrency: number): MemoizedFunc<T> {
  const locks: CachedPromises<T> = {}
  const limit = pLimit(concurrency)

  return function (key: string, fn: () => Promise<T>): Promise<T> {
    if (locks[key]) return locks[key]
    locks[key] = limit(fn)
    return locks[key]
  }
}
