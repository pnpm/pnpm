import crypto from 'node:crypto'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

const OWNER_FILE = 'owner'
const POLL_INTERVAL_MS = 50

export interface DirLockOptions {
  waitMs: number
  // Bounds how long a holder whose process cannot be checked may keep the lock.
  abandonedMs: number
}

/**
 * A cross-process advisory lock: creating a directory is atomic, so the winner
 * of the `mkdir` race holds it. A lock whose holder process on this host has
 * ended, or that is older than `abandonedMs`, is taken over.
 */
export class DirLock {
  private readonly lockPath: string
  private readonly token: string

  private constructor (lockPath: string, token: string) {
    this.lockPath = lockPath
    this.token = token
  }

  /** Resolves to `undefined` when the lock is still held after `waitMs`. */
  static async acquire (lockPath: string, opts: DirLockOptions): Promise<DirLock | undefined> {
    return DirLock.acquireUntil(lockPath, Date.now() + opts.waitMs, opts.abandonedMs)
  }

  private static async acquireUntil (lockPath: string, deadline: number, abandonedMs: number): Promise<DirLock | undefined> {
    try {
      await fs.mkdir(lockPath)
      const token = `${os.hostname()}:${process.pid}:${Date.now()}:${crypto.randomUUID()}`
      try {
        await fs.writeFile(path.join(lockPath, OWNER_FILE), token, { mode: 0o600 })
      } catch (err: unknown) {
        await fs.rm(lockPath, { force: true, recursive: true }).catch(() => {})
        throw err
      }
      return new DirLock(lockPath, token)
    } catch (err: unknown) {
      if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
    }
    const stats = await fs.lstat(lockPath).catch(() => undefined)
    if (stats == null || stats.isSymbolicLink() || !stats.isDirectory()) return undefined
    if (Date.now() - stats.mtimeMs > abandonedMs || await isHeldByEndedProcess(lockPath)) {
      const removed = await fs.rm(lockPath, { force: true, recursive: true }).then(() => true, () => false)
      if (removed) return DirLock.acquireUntil(lockPath, deadline, abandonedMs)
    }
    if (Date.now() >= deadline) return undefined
    await new Promise(resolve => setTimeout(resolve, POLL_INTERVAL_MS))
    return DirLock.acquireUntil(lockPath, deadline, abandonedMs)
  }

  async isOwner (): Promise<boolean> {
    try {
      return await fs.readFile(path.join(this.lockPath, OWNER_FILE), 'utf8') === this.token
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
      throw err
    }
  }

  async release (): Promise<void> {
    if (!await this.isOwner().catch(() => false)) return
    await fs.rm(this.lockPath, { force: true, recursive: true }).catch(() => {})
  }
}

async function isHeldByEndedProcess (lockPath: string): Promise<boolean> {
  const owner = await fs.readFile(path.join(lockPath, OWNER_FILE), 'utf8').catch(() => undefined)
  if (owner == null) return false
  const [hostname, pidText] = owner.split(':')
  const pid = Number(pidText)
  if (hostname !== os.hostname() || !Number.isInteger(pid) || pid <= 0 || pid === process.pid) return false
  try {
    process.kill(pid, 0)
    return false
  } catch (err: unknown) {
    return util.types.isNativeError(err) && 'code' in err && err.code === 'ESRCH'
  }
}
