import logger from '@pnpm/logger'
import storePath from '@pnpm/store-path'
import {
  storePrune,
  storeStatus,
} from 'supi'
import createStoreController from '../createStoreController'
import {PnpmError} from '../errorTypes'
import {PnpmOptions} from '../types'
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
      const store = await createStoreController(opts)
      const storePruneOptions = Object.assign(opts, {
        store: store.path,
        storeController: store.ctrl,
      })
      return storePrune(storePruneOptions)
    default:
      help(['store'])
      if (input[0]) {
        const err = new Error(`"store ${input[0]}" is not a pnpm command. See "pnpm help store".`)
        err['code'] = 'ERR_PNPM_INVALID_STORE_COMMAND' // tslint:disable-line:no-string-literal
        throw err
      }
  }
}

async function statusCmd (opts: PnpmOptions) {
  const modifiedPkgs = await storeStatus(Object.assign(opts, {
    store: await storePath(opts.prefix, opts.store),
  }))
  if (!modifiedPkgs || !modifiedPkgs.length) {
    logger.info('Packages in the store are untouched')
    return
  }

  throw new StoreStatusError(modifiedPkgs)
}
