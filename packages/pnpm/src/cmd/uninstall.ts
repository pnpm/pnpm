import logger from '@pnpm/logger'
import {
  uninstall,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmOptions} from '../types'

export default async function uninstallCmd (
  input: string[],
  opts: PnpmOptions,
) {
  const store = await createStoreController(opts)
  const uninstallOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  return uninstall(input, uninstallOpts)
}
