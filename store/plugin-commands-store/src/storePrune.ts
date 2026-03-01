import { cleanOrphanedInstallDirs } from '@pnpm/global.packages'
import { streamParser } from '@pnpm/logger'
import { type StoreController } from '@pnpm/store-controller-types'
import { type ReporterFunction } from './types.js'
import { cleanExpiredDlxCache } from './cleanExpiredDlxCache.js'

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
