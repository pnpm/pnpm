import crypto from 'node:crypto'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

const OWNER_FILE = 'owner'
const POLL_INTERVAL_MS = 50
// A holder writes its owner file right after creating the directory, so one
// that still has none after this long died in between.
const OWNERLESS_ABANDONED_MS = 5_000
// Reaping is a few file operations, so a reaper lock this old was left behind.
const REAPER_ABANDONED_MS = 10_000

export interface DirLockOptions {
  waitMs: number
  // Bounds how long a holder whose process cannot be checked may keep the lock.
  abandonedMs: number
}

/**
 * A cross-process advisory lock: creating a directory is atomic, so the winner
 * of the `mkdir` race holds it. A lock is taken over once its holder, another
 * process on this host, has ended; any other holder's lock once it is older
 * than `abandonedMs`.
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
    const deadline = Date.now() + opts.waitMs
    for (;;) {
      // eslint-disable-next-line no-await-in-loop
      const lock = await DirLock.tryCreate(lockPath)
      if (lock != null) return lock
      // eslint-disable-next-line no-await-in-loop
      const state = await inspectLock(lockPath, opts.abandonedMs)
      if (state.kind === 'unusable') return undefined
      // Released since the `mkdir` attempt: retry at once.
      if (state.kind === 'vanished' && Date.now() < deadline) continue
      // eslint-disable-next-line no-await-in-loop
      if (state.kind === 'stale' && await removeIfStillStale(lockPath, state.owner, opts.abandonedMs)) continue
      if (Date.now() >= deadline) return undefined
      // eslint-disable-next-line no-await-in-loop
      await new Promise(resolve => setTimeout(resolve, POLL_INTERVAL_MS))
    }
  }

  async isOwner (): Promise<boolean> {
    return await readOwner(this.lockPath) === this.token
  }

  async release (): Promise<void> {
    if (!await this.isOwner().catch(() => false)) return
    await fs.rm(this.lockPath, { force: true, recursive: true }).catch(() => {})
  }

  private static async tryCreate (lockPath: string): Promise<DirLock | undefined> {
    try {
      await fs.mkdir(lockPath)
    } catch (err: unknown) {
      if (isErrorCode(err, 'EEXIST')) return undefined
      throw err
    }
    const token = `${os.hostname()}:${process.pid}:${Date.now()}:${crypto.randomUUID()}`
    try {
      await fs.writeFile(path.join(lockPath, OWNER_FILE), token, { mode: 0o600 })
    } catch (err: unknown) {
      await fs.rm(lockPath, { force: true, recursive: true }).catch(() => {})
      throw err
    }
    return new DirLock(lockPath, token)
  }
}

type LockState =
  | { kind: 'live' | 'vanished' | 'unusable' }
  | { kind: 'stale', owner: string | undefined }

async function inspectLock (lockPath: string, abandonedMs: number): Promise<LockState> {
  let stats
  try {
    stats = await fs.lstat(lockPath)
  } catch (err: unknown) {
    if (isErrorCode(err, 'ENOENT')) return { kind: 'vanished' }
    throw err
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) return { kind: 'unusable' }
  const owner = await readOwner(lockPath)
  const age = Date.now() - stats.mtimeMs
  const localPid = owner == null ? undefined : localOwnerPid(owner)
  let stale: boolean
  if (owner == null) {
    stale = age > OWNERLESS_ABANDONED_MS
  } else if (localPid != null && localPid !== process.pid) {
    // Another process on this host proves its liveness, so its age does not matter.
    stale = hasEnded(localPid)
  } else {
    stale = age > abandonedMs
  }
  return stale ? { kind: 'stale', owner } : { kind: 'live' }
}

/**
 * Removes the lock only while it still carries the stale owner, under a reaper
 * lock, so that a waiter never removes a lock another waiter just took over.
 */
async function removeIfStillStale (lockPath: string, staleOwner: string | undefined, abandonedMs: number): Promise<boolean> {
  const reaperPath = `${lockPath}.reap`
  try {
    await fs.mkdir(reaperPath)
  } catch (err: unknown) {
    if (!isErrorCode(err, 'EEXIST')) throw err
    const stats = await fs.lstat(reaperPath).catch(() => undefined)
    if (stats != null && Date.now() - stats.mtimeMs > REAPER_ABANDONED_MS) {
      await fs.rm(reaperPath, { force: true, recursive: true }).catch(() => {})
    }
    return false
  }
  try {
    const current = await inspectLock(lockPath, abandonedMs)
    if (current.kind !== 'stale' || current.owner !== staleOwner) return false
    return await fs.rm(lockPath, { force: true, recursive: true }).then(() => true, () => false)
  } finally {
    await fs.rmdir(reaperPath).catch(() => {})
  }
}

async function readOwner (lockPath: string): Promise<string | undefined> {
  try {
    return await fs.readFile(path.join(lockPath, OWNER_FILE), 'utf8')
  } catch (err: unknown) {
    if (isErrorCode(err, 'ENOENT')) return undefined
    throw err
  }
}

function localOwnerPid (owner: string): number | undefined {
  const [hostname, pidText] = owner.split(':')
  const pid = Number(pidText)
  return hostname === os.hostname() && Number.isInteger(pid) && pid > 0 ? pid : undefined
}

function hasEnded (pid: number): boolean {
  try {
    process.kill(pid, 0)
    return false
  } catch (err: unknown) {
    return isErrorCode(err, 'ESRCH')
  }
}

function isErrorCode (err: unknown, code: string): boolean {
  return util.types.isNativeError(err) && 'code' in err && err.code === code
}
