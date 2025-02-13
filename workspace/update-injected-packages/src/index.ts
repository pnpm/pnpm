import { PnpmError } from '@pnpm/error'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'
import { injectedPackages } from './injectedPackages'

export interface UpdateInjectedPackagesOptions {
  pkgRootDir: string
  virtualStoreDir: string | undefined
}

export async function updateInjectedPackages (opts: UpdateInjectedPackagesOptions): Promise<void> {
  if (!opts.virtualStoreDir) {
    throw new PnpmError('NO_VIRTUAL_STORE_DIR', 'Cannot update injected packages without virtual store dir')
  }
  const lockfile = await readCurrentLockfile(opts.virtualStoreDir, {
    ignoreIncompatible: false,
  })
  if (!lockfile) return
  for (const info of injectedPackages(lockfile)) {
    console.log('INJECTED PACKAGE', info) // TODO: remove this later
    // TODO: continue from here
  }
}
