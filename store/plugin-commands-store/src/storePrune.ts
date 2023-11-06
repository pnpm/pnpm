import { streamParser } from '@pnpm/logger'
import { type StoreController } from '@pnpm/store-controller-types'
import { type ReporterFunction } from './types'

export async function storePrune (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
    removeAlienFiles?: boolean
  }
) {
  const reporter = opts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  await opts.storeController.prune(opts.removeAlienFiles)
  await opts.storeController.close()

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}
