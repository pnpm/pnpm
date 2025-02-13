import path from 'path'
import { PnpmError } from '@pnpm/error'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'
import { globalInfo } from '@pnpm/logger'
import { findInjectedPackages } from './findInjectedPackages'

export interface UpdateInjectedPackagesOptions {
  pkgName: string | undefined
  pkgRootDir: string
  virtualStoreDir: string | undefined
  virtualStoreDirMaxLength: number
  workspaceDir: string | undefined
}

export async function updateInjectedPackages (opts: UpdateInjectedPackagesOptions): Promise<void> {
  if (!opts.pkgName) {
    globalInfo(`Skip updating ${opts.pkgRootDir} as an injected package because without name, it cannot be a dependency`)
    return
  }
  if (!opts.virtualStoreDir) {
    throw new PnpmError('NO_VIRTUAL_STORE_DIR', 'Cannot update injected packages without virtual store dir')
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('NO_WORKSPACE_DIR', 'Cannot update injected packages without workspace dir')
  }
  const lockfile = await readCurrentLockfile(opts.virtualStoreDir, {
    ignoreIncompatible: false,
  })
  if (!lockfile) {
    globalInfo('Stop updating injected packages because no current lockfile means no dependency')
    return
  }
  const pkgRootDir = path.resolve(opts.workspaceDir, opts.pkgRootDir)
  for (const info of findInjectedPackages({
    lockfile,
    pkgName: opts.pkgName,
    pkgRootDir,
    workspaceDir: opts.workspaceDir,
  })) {
    console.log('INJECTED PACKAGE', info) // TODO: remove this later
    // TODO: continue from here
  }
}
