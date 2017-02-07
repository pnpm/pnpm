import logger from 'pnpm-logger'
import path = require('path')
import lockfile = require('proper-lockfile')

async function delay(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

async function lock(
  lockFilename: string,
  opts: {firstTime: boolean, stale: number}
): Promise<{}> {
  const promise = new Promise((resolve, reject) => {
    lockfile.lock(
      lockFilename,
      {realpath: false, stale: opts.stale},
      async (err: Error & {code: string}) => {
        if (err && err.code === 'ELOCKED') {
          if (opts.firstTime) {
            logger.warn('waiting for another installation to complete...')
          }
          await delay(200)
          await lock(lockFilename, {firstTime: false, stale: opts.stale})
          resolve()
        } else if (err) {
          reject(err)
        } else {
          resolve()
        }
      })
  })
  return promise as Promise<{}>
}

async function unlock(lockFilename: string): Promise<{}> {
  const promise = new Promise(resolve =>
    lockfile.unlock(
      lockFilename,
      {realpath: false},
      resolve))
  return promise as Promise<{}>
}

export default async function withLock<T> (
  storePath: string,
  fn: () => Promise<T>,
  opts: {stale: number}
): Promise<T> {
  const lockFilename: string = path.resolve(storePath, 'lock')
  await lock(lockFilename, {firstTime: true, stale: opts.stale})
  try {
    const result = await fn()
    await unlock(lockFilename)
    return result
  } catch (err) {
    await unlock(lockFilename)
    throw err;
  }
}
