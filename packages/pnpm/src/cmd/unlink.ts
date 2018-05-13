import {
  unlink,
  unlinkPkgs,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmOptions} from '../types'

export default async function (input: string[], opts: PnpmOptions) {
  const store = await createStoreController(opts)
  const unlinkOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (!input || !input.length) {
    return unlink(unlinkOpts)
  }
  return unlinkPkgs(input, unlinkOpts)
}
