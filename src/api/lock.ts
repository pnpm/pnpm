import lock from '@pnpm/fs-locker'
import logger from '@pnpm/logger'

export default async function withLock<T> (
  dir: string,
  fn: () => Promise<T>,
  opts: {
    stale: number,
    locks: string,
  },
): Promise<T> {
  const unlock = await lock(dir, {
    locks: opts.locks,
    stale: opts.stale,
    whenLocked () {
      logger.warn('waiting for another installation to complete...')
    },
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
