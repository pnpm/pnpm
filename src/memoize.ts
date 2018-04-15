import pLimit = require('p-limit')

export type MemoizedFunc<T> = (key: string, fn: () => Promise<T>) => Promise<T>

/**
 * Save promises for later
 */
export default function memoize <T> (concurrency?: number): MemoizedFunc<T> {
  const locks = new Map<string, Promise<T>>()
  const limit = concurrency && pLimit(concurrency) as (fn: () => Promise<T>) => Promise<T>

  return (key: string, fn: () => Promise<T>): Promise<T> => {
    let v = locks.get(key)
    if (v) return v
    v = limit && limit(fn) || fn()
    locks.set(key, v)
    return v.catch((err: Error) => {
      locks.delete(key)
      throw err
    })
  }
}
