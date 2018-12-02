import { mutateModules } from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export default async function (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (!input || !input.length) {
    return mutateModules([
      {
        dependencyNames: input,
        mutation: 'unlinkSome',
        prefix: opts.prefix,
      },
    ], unlinkOpts)
  }
  return mutateModules([
    {
      mutation: 'unlink',
      prefix: opts.prefix,
    },
  ], unlinkOpts)
}
