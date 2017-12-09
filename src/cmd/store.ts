import logger from '@pnpm/logger'
import {
  PnpmOptions,
  storePrune,
  storeStatus,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmError} from '../errorTypes'
import help from './help'

class StoreStatusError extends PnpmError {
  public modified: string[]
  constructor (modified: string[]) {
    super('MODIFIED_DEPENDENCY', '')
    this.modified = modified
  }
}

export default async function (input: string[], opts: PnpmOptions) {
  switch (input[0]) {
    case 'status':
      return statusCmd(opts)
    case 'prune':
      opts['storeController'] = await createStoreController(opts) // tslint:disable-line
      return storePrune(opts)
    default:
      help(['store'])
  }
}

async function statusCmd (opts: PnpmOptions) {
  const modifiedPkgs = await storeStatus(opts)
  if (!modifiedPkgs || !modifiedPkgs.length) {
    logger.info('Packages in the store are untouched')
    return
  }

  throw new StoreStatusError(modifiedPkgs)
}
