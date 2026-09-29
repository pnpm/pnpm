import { cleanOrphanedInstallDirs } from '@pnpm/global.packages'
import { streamParser } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'

import { cleanExpiredDlxCache } from './cleanExpiredDlxCache.js'
import type { ReporterFunction } from './types.js'

export async function storePrune (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
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

  try {
    // The dlx cache holds hard links into the store and registers its
    // installs as projects, so it is cleaned first to let this prune
    // reclaim what the expired entries used.
    await cleanExpiredDlxCache({
      cacheDir: opts.cacheDir,
      dlxCacheMaxAge: opts.dlxCacheMaxAge,
      now: new Date(),
    })

    await opts.storeController.prune(opts.removeAlienFiles)

    if (opts.globalPkgDir) {
      cleanOrphanedInstallDirs(opts.globalPkgDir)
    }
  } finally {
    await opts.storeController.close()
    if ((reporter != null) && typeof reporter === 'function') {
      streamParser.removeListener('data', reporter)
    }
  }
}
