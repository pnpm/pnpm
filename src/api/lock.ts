import logger from '@pnpm/logger'
import lock from '@pnpm/fs-locker'

export default async function withLock<T> (
  dir: string,
  fn: () => Promise<T>,
  opts: {
    stale: number,
    locks: string,
  }
): Promise<T> {
  const unlock = await lock(dir, {
    stale: opts.stale,
    locks: opts.locks,
    whenLocked () {
      logger.warn('waiting for another installation to complete...')
    }
  })
  try {
    const result = await fn()
    await unlock()
    return result
  } catch (err) {
    await unlock()
    throw err;
  }
}
