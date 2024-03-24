import { streamParser } from '@pnpm/logger'
import type { StoreController } from '@pnpm/types'

import type { ReporterFunction } from './types.js'

export async function storePrune(opts: {
  reporter?: ReporterFunction | undefined
  storeController: StoreController
  removeAlienFiles?: boolean | undefined
}): Promise<void> {
  const reporter = opts?.reporter

  if (reporter != null && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  await opts.storeController.prune(opts.removeAlienFiles)

  await opts.storeController.close()

  if (reporter != null && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}
