import lock from '@pnpm/fs-locker'
import logger from '@pnpm/logger'

export default async function withLock<T> (
  dir: string,
  fn: () => Promise<T>,
  opts: {
    stale: number,
    locks: string,
    prefix: string,
  },
): Promise<T> {
  const unlock = await lock(dir, {
    locks: opts.locks,
    stale: opts.stale,
    whenLocked () {
      logger.warn({
        message: 'waiting for another installation to complete...',
        prefix: opts.prefix,
      })
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
