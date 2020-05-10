import { streamParser } from '@pnpm/logger'
import { StoreController } from '@pnpm/store-controller-types'
import { ReporterFunction } from './types'

export default async function (
  opts: {
    reporter?: ReporterFunction,
    storeController: StoreController,
  },
) {
  const reporter = opts?.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  await opts.storeController.prune()
  await opts.storeController.close()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
