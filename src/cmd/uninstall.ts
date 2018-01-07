import logger from '@pnpm/logger'
import {
  uninstall,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmOptions} from '../types'

export default async function uninstallCmd (
  input: string[],
  opts: PnpmOptions,
  cmdName: string,
) {
  const store = await createStoreController(opts)
  const uninstallOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  if (cmdName === 'unlink') {
    logger.warn('This command will behave as `pnpm dislink` in the future')
  }
  return uninstall(input, uninstallOpts)
}
