import { readWantedLockfile } from '@pnpm/lockfile-file'
import type { Lockfile } from '@pnpm/lockfile-types'

export async function loadLockfile (pkgPath: string): Promise<Lockfile> {
  const lockfile = await readWantedLockfile(pkgPath, {
    ignoreIncompatible: false,
  })
  if (!lockfile) {
    throw Error('should lockfile successfully')
  } else {
    return lockfile
  }
}
