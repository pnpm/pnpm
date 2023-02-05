import type { Lockfile } from '@pnpm/lockfile-types'
import { mergeLockfileChanges } from '@pnpm/merge-lockfile-changes'
import { pickLockfileInfo } from './utils'
import { createLockfileObject } from '@pnpm/lockfile-file'

const EMPTY_LOCKFILE: Lockfile = createLockfileObject([], {})

export function mergeSplittedLockfiles (lockfiles: Record<string, Lockfile>): Lockfile {
  const entires = Object.entries(lockfiles)
  if (entires.length === 0) {
    return EMPTY_LOCKFILE
  }
  const newLockfile = entires.reduce((prev, current) => {
    const importers = {
      [current[0]]: current[1].importers['.'],
    }
    const lockfile = {
      ...current[1],
      importers,
    }
    return mergeLockfileChanges(prev, lockfile)
  }, EMPTY_LOCKFILE)
  const rootLockfile = lockfiles['.']
  if (!rootLockfile) {
    throw Error('Can\'t find the root lockfiles')
  } else {
    return {
      ...pickLockfileInfo(rootLockfile),
      ...newLockfile,
    }
  }
}
