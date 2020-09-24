import { streamParser } from '@pnpm/logger'
import { StoreController } from '@pnpm/store-controller-types'
import { ReporterFunction } from './types'

export default async function (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
  }
) {
  const reporter = opts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  await opts.storeController.prune()
  await opts.storeController.close()

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}
