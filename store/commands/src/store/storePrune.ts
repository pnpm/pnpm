import { cleanOrphanedInstallDirs, scanGlobalPackages } from '@pnpm/global.packages'
import { streamParser } from '@pnpm/logger'
import { registerProject } from '@pnpm/store.controller'
import type { StoreController } from '@pnpm/store.controller-types'

import { cleanExpiredDlxCache } from './cleanExpiredDlxCache.js'
import type { ReporterFunction } from './types.js'

export async function storePrune (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
    storeDir: string
    removeAlienFiles?: boolean
    cacheDir: string
    dlxCacheMaxAge: number
    globalPkgDir?: string
  }
): Promise<void> {
  const reporter = opts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  // Ensure all global package install dirs are registered before pruning,
  // so their packages in the global virtual store are not removed.
  if (opts.globalPkgDir) {
    await registerGlobalPackageProjects(opts.storeDir, opts.globalPkgDir)
  }

  await opts.storeController.prune(opts.removeAlienFiles)
  await opts.storeController.close()

  await cleanExpiredDlxCache({
    cacheDir: opts.cacheDir,
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    now: new Date(),
  })

  if (opts.globalPkgDir) {
    cleanOrphanedInstallDirs(opts.globalPkgDir)
  }

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}

async function registerGlobalPackageProjects (storeDir: string, globalPkgDir: string): Promise<void> {
  const packages = scanGlobalPackages(globalPkgDir)
  await Promise.all(
    packages.map(({ installDir }) => registerProject(storeDir, installDir))
  )
}
