interface CachedPromises<T> {
  [name: string]: Promise<T>
}

export type LockedFunc<T> = (key: string, fn: () => Promise<T>) => Promise<T>

/**
 * Save promises for later
 */
export function locking<T>(): LockedFunc<T | undefined> {
  const locks: CachedPromises<T | undefined> = {}

  return async (key: string, fn: () => Promise<T | undefined>): Promise<T | undefined> => {
    if (locks[key] != null) {
      return locks[key]
    }

    locks[key] = fn()

    fn().then(
      (): boolean => {
        return delete locks[key];
      },
      (): boolean => {
        return delete locks[key];
      }
    )

    return locks[key]
  }
}
