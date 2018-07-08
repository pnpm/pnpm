import logger from '@pnpm/logger'
import storePath from '@pnpm/store-path'
import {
  storeAdd,
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
  let store;
  switch (input[0]) {
    case 'status':
      return statusCmd(opts)
    case 'prune':
      store = await createStoreController(opts)
      const storePruneOptions = Object.assign(opts, {
        store: store.path,
        storeController: store.ctrl,
      })
      return storePrune(storePruneOptions)
    case 'add':
      store = await createStoreController(opts);
      return storeAdd(input.slice(1), {
        prefix: opts.prefix,
        registry: opts.registry,
        reporter: opts.reporter,
        storeController: store.ctrl,
        tag: opts.tag,
        verifyStoreIntegrity: opts.verifyStoreIntegrity,
      });
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
    logger.info({
      message: 'Packages in the store are untouched',
      prefix: opts.prefix,
    })
    return
  }

  throw new StoreStatusError(modifiedPkgs)
}
