import type { LockfileObject } from '@pnpm/lockfile.types'
import { clone } from 'ramda'

/**
 * The update runs on a disposable clone: it may mutate freely and return
 * `false` at any point, and nothing reaches `lockfile` unless the candidate
 * passes every gate.
 */
export async function tryFastUpdateLockfile (
  lockfile: LockfileObject,
  opts: {
    update: (candidate: LockfileObject) => boolean | Promise<boolean>
    isLockfileUpToDate: (candidate: LockfileObject) => Promise<boolean>
    verifyLockfile?: (candidate: LockfileObject) => Promise<void>
  }
): Promise<boolean> {
  const candidate = clone(lockfile)
  if (!await opts.update(candidate)) return false
  if (!await opts.isLockfileUpToDate(candidate)) return false
  await opts.verifyLockfile?.(candidate)

  // The candidate replaces the lockfile wholesale: a key the update deleted
  // (an emptied `packages` or `catalogs` section) has to disappear too, which
  // assignment alone would not do.
  for (const key of Object.keys(lockfile)) {
    if (!Object.hasOwn(candidate, key)) {
      delete lockfile[key as keyof LockfileObject]
    }
  }
  Object.assign(lockfile, candidate)
  return true
}
