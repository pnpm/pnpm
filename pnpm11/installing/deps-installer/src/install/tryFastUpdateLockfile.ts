import type { LockfileObject } from '@pnpm/lockfile.types'
import { clone } from 'ramda'

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

  Object.assign(lockfile, candidate)
  return true
}
