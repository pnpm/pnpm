import fs from 'node:fs'
import util, { promisify } from 'node:util'

import gfs from 'graceful-fs'

const RENAME_RETRY_BUDGET_MS = 60_000
const RENAME_RETRY_BACKOFF_CAP_MS = 100
const renameRetrySleepBuffer = new Int32Array(new SharedArrayBuffer(4))

export default { // eslint-disable-line
  chmod: promisify(gfs.chmod),
  copyFile: promisify(gfs.copyFile),
  copyFileSync: withEagainRetry(gfs.copyFileSync),
  createReadStream: gfs.createReadStream,
  link: promisify(gfs.link),
  linkSync: withEagainRetry(gfs.linkSync),
  mkdir: promisify(gfs.mkdir),
  mkdirSync: withEagainRetry(gfs.mkdirSync),
  renameSync: withEagainRetry(gfs.renameSync),
  readFile: promisify(gfs.readFile),
  readFileSync: gfs.readFileSync,
  readdirSync: gfs.readdirSync,
  stat: promisify(gfs.stat),
  statSync: gfs.statSync,
  unlink: promisify(gfs.unlink),
  unlinkSync: gfs.unlinkSync,
  writeFile: promisify(gfs.writeFile),
  writeFileSync: withEagainRetry(gfs.writeFileSync),
}

function withEagainRetry<T extends unknown[], R> (
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

/**
 * Renames `src` over `dest`, waiting out a Windows sharing violation — an
 * EPERM, EACCES or EBUSY from whoever else holds the file open — for up to a
 * minute before rethrowing it. Every other error is thrown right away.
 *
 * `dest` is never removed to make room for the rename: a concurrent install may
 * still be reading that dirent, and a reader has to see either the whole file
 * that was there or the whole file replacing it.
 */
export function renameFileWithRetry (src: string, dest: string): void {
  const startedAt = Date.now()
  let backoffMs = 0
  for (;;) {
    try {
      fs.renameSync(src, dest)
      return
    } catch (err) {
      if (!isTransientRenameError(err) || Date.now() - startedAt >= RENAME_RETRY_BUDGET_MS) throw err
      if (backoffMs > 0) Atomics.wait(renameRetrySleepBuffer, 0, 0, backoffMs)
      backoffMs = Math.min(backoffMs + 10, RENAME_RETRY_BACKOFF_CAP_MS)
    }
  }
}

function isTransientRenameError (err: unknown): boolean {
  return process.platform === 'win32' &&
    util.types.isNativeError(err) &&
    'code' in err &&
    (err.code === 'EPERM' || err.code === 'EACCES' || err.code === 'EBUSY')
}
