import logger from 'pnpm-logger'
import path = require('path')
import lockfile = require('proper-lockfile')

async function delay(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

async function lock(lockFilename: string, notFirstTime: boolean = false): Promise<{}> {
  const promise = new Promise((resolve, reject) => {
    lockfile.lock(
      lockFilename,
      {realpath: false},
      async (err: Error & {code: string}) => {
        if (err) {
          if (err.code === 'ELOCKED') {
            if (!notFirstTime) {
              logger.warn('waiting for another installation to complete...')
            }
            await delay(200)
            await lock(lockFilename, true)
            resolve()
          } else {
            reject(err)
          }
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

export default async function withLock<T> (storePath: string, fn: () => Promise<T>): Promise<T> {
  const lockFilename: string = path.resolve(storePath, 'lock')
  await lock(lockFilename)
  try {
    const result = await fn()
    await unlock(lockFilename)
    return result
  } catch (err) {
    await unlock(lockFilename)
    throw err;
  }
}
