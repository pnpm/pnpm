import util, { promisify } from 'util'
import fs from 'fs'
import gfs from 'graceful-fs'

export function withEagainRetry<T extends unknown[], R> (
  fn: (...args: T) => R,
  maxRetries: number = 15
): (...args: T) => R {
  return (...args: T): R => {
    let attempts = 0
    while (attempts <= maxRetries) {
      try {
        return fn(...args)
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'EAGAIN' && attempts < maxRetries) {
          attempts++
          // Exponential backoff: wait 2^attempts milliseconds, max 300ms
          const delay = Math.min(Math.pow(2, attempts), 300)
          Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, delay)
          continue
        }
        throw err
      }
    }
    throw new Error('Unreachable')
  }
}

export const renameSyncWithRetry = withEagainRetry(fs.renameSync)
export const mkdirSyncWithRetry = withEagainRetry(fs.mkdirSync)
export const writeFileWithRetry = withEagainRetry(fs.writeFileSync)
export const linkSyncWithRetry = withEagainRetry(fs.linkSync)
export const copyFileWithRetry = withEagainRetry(fs.copyFileSync)

export default { // eslint-disable-line
  copyFile: promisify(gfs.copyFile),
  copyFileSync: gfs.copyFileSync,
  createReadStream: gfs.createReadStream,
  link: promisify(gfs.link),
  linkSync: gfs.linkSync,
  readFile: promisify(gfs.readFile),
  readFileSync: gfs.readFileSync,
  readdirSync: gfs.readdirSync,
  stat: promisify(gfs.stat),
  statSync: gfs.statSync,
  unlinkSync: gfs.unlinkSync,
  writeFile: promisify(gfs.writeFile),
  writeFileSync: gfs.writeFileSync,
}
