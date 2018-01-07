import {prune} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmOptions} from '../types'

export default async (input: string[], opts: PnpmOptions) => {
  const store = await createStoreController(opts)
  const pruneOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })
  return prune(pruneOpts)
}
