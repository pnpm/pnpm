import {PnpmOptions} from '@pnpm/types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import {streamParser} from '@pnpm/logger'

export default async function (maybeOpts: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  await ctx.storeController.prune()
  await ctx.storeController.saveState()
  await ctx.storeController.close()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
