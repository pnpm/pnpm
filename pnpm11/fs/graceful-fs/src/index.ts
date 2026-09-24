import fs from 'node:fs'
import os from 'node:os'
import util, { promisify } from 'node:util'

import gfs from 'graceful-fs'

const FILE_LOCK_RETRY_BUDGET_MS = 60_000
const PERMISSION_DENIED_RETRY_BUDGET_MS = 1_000
const FILE_LOCK_RETRY_BACKOFF_CAP_MS = 100
const fileLockRetrySleepBuffer = new Int32Array(new SharedArrayBuffer(4))

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
 * Renames `src` over `dest`, retrying Windows EBUSY errors for up to a minute.
 * Windows drives mounted into WSL get the same retries.
 * EPERM and EACCES have a one-second budget because they can also indicate
 * permanent permission or destination conflicts. Other errors are thrown
 * right away.
 *
 * `dest` is never removed to make room for the rename: a concurrent install may
 * still be reading that dirent, and a reader has to see either the whole file
 * that was there or the whole file replacing it.
 */
export function renameFileWithRetry (src: string, dest: string): void {
  withFileLockRetry(() => {
    fs.renameSync(src, dest)
  })
}

/**
 * Asynchronous {@link renameFileWithRetry}, which waits between attempts
 * without blocking the event loop.
 */
export async function renameFileWithRetryAsync (src: string, dest: string): Promise<void> {
  await withFileLockRetryAsync(() => fs.promises.rename(src, dest))
}

/**
 * Reads `target`'s stats without following it, with the retry policy of
 * {@link renameFileWithRetry}.
 *
 * A Windows path another process has just unlinked stays delete-pending until
 * the last handle on it closes, and inspecting it fails with EPERM for as long
 * as that lasts. Retrying lets the unlink land, so a caller that reads ENOENT
 * as an absent entry sees the same absence POSIX shows it at once.
 */
export function lstatWithRetry (target: string): fs.Stats {
  return withFileLockRetry(() => fs.lstatSync(target))
}

/**
 * Removes a file, with the retry policy of {@link renameFileWithRetry}.
 */
export function unlinkWithRetry (target: string): void {
  withFileLockRetry(() => {
    fs.unlinkSync(target)
  })
}

/**
 * Runs a filesystem operation with the retry policy of
 * {@link renameFileWithRetry}.
 */
export function withFileLockRetry<T> (operation: () => T): T {
  const retry = new FileLockRetry()
  for (;;) {
    try {
      return operation()
    } catch (err) {
      const delayMs = retry.delayBeforeNextAttempt(err)
      if (delayMs > 0) Atomics.wait(fileLockRetrySleepBuffer, 0, 0, delayMs)
      retry.checkBudgetAfterDelay(err)
    }
  }
}

async function withFileLockRetryAsync<T> (operation: () => Promise<T>): Promise<T> {
  const retry = new FileLockRetry()
  for (;;) {
    try {
      // eslint-disable-next-line no-await-in-loop
      return await operation()
    } catch (err) {
      const delayMs = retry.delayBeforeNextAttempt(err)
      if (delayMs > 0) {
        // eslint-disable-next-line no-await-in-loop
        await new Promise((resolve) => setTimeout(resolve, delayMs))
      }
      retry.checkBudgetAfterDelay(err)
    }
  }
}

class FileLockRetry {
  private readonly startedAt = Date.now()
  private backoffMs = 0
  private budgetMs = FILE_LOCK_RETRY_BUDGET_MS

  /** Rethrows `err` unless another attempt fits the budget; returns the delay before it. */
  delayBeforeNextAttempt (err: unknown): number {
    if (!isTransientFileLockError(err)) throw err
    if (err.code === 'EPERM' || err.code === 'EACCES') this.budgetMs = Math.min(this.budgetMs, PERMISSION_DENIED_RETRY_BUDGET_MS)
    const remainingMs = this.budgetMs - (Date.now() - this.startedAt)
    if (remainingMs <= 0) throw err
    return Math.min(this.backoffMs, remainingMs)
  }

  checkBudgetAfterDelay (err: unknown): void {
    if (Date.now() - this.startedAt >= this.budgetMs) throw err
    this.backoffMs = Math.min(this.backoffMs + 10, FILE_LOCK_RETRY_BACKOFF_CAP_MS)
  }
}

function isTransientFileLockError (err: unknown): err is NodeJS.ErrnoException {
  return (process.platform === 'win32' || isWsl()) &&
    util.types.isNativeError(err) &&
    'code' in err &&
    (err.code === 'EPERM' || err.code === 'EACCES' || err.code === 'EBUSY')
}

// A Windows drive mounted into WSL (/mnt/c) keeps Windows file locking, so an
// antivirus or indexer handle fails a rename there with EACCES. WSL kernels
// carry "microsoft" in their release, e.g. 5.15.167.4-microsoft-standard-WSL2.
function isWsl (): boolean {
  return process.platform === 'linux' && os.release().toLowerCase().includes('microsoft')
}
