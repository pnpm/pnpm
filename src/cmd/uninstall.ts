import logger from '@pnpm/logger'
import {PnpmOptions, uninstall} from 'supi'
import createStoreController from '../createStoreController'

export default async function uninstallCmd (input: string[], opts: PnpmOptions, cmdName: string) {
  opts['storeController'] = (await createStoreController(opts)).ctrl // tslint:disable-line

  if (cmdName === 'unlink') {
    logger.warn('This command will behave as `pnpm dislink` in the future')
  }
  return uninstall(input, opts)
}
