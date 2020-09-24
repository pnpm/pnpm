interface CachedPromises<T> {
  [name: string]: Promise<T>
}

export type LockedFunc<T> = (key: string, fn: () => Promise<T>) => Promise<T>

/**
 * Save promises for later
 */
export default function lock<T> (): LockedFunc<T> {
  const locks: CachedPromises<T> = {}

  return (key: string, fn: () => Promise<T>): Promise<T> => {
    if (locks[key]) return locks[key]
    locks[key] = fn()
    fn()
      .then(() => delete locks[key], () => delete locks[key])
    return locks[key]
  }
}
