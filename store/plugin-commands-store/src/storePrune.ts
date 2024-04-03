import { streamParser } from '@pnpm/logger'
import { type StoreController } from '@pnpm/store-controller-types'
import { type ReporterFunction } from './types'
import { cleanExpiredDlxCache } from './cleanExpiredDlxCache'

export async function storePrune (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
    removeAlienFiles?: boolean
    cacheDir: string
    dlxCacheMaxAge: number
  }
) {
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

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}
