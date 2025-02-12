import { PnpmError } from '@pnpm/error'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'

export interface UpdateInjectedPackagesOptions {
  packageDir: string
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
  // TODO: continue from here
}

// TODO: write a function that lists all injected dependencies of a workspace package
