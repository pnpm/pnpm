import { docsUrl } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import logger, { LogBase } from '@pnpm/logger'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import storePath from '@pnpm/store-path'
import storeAdd from './storeAdd'
import storePrune from './storePrune'
import storeStatus from './storeStatus'
import R = require('ramda')
import renderHelp = require('render-help')

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'registry',
    'store',
    'store-dir',
  ], allTypes)
}

export const commandNames = ['store']

export function help () {
  return renderHelp({
    description: 'Reads and performs actions on pnpm store that is on the current filesystem.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: '\
Checks for modified packages in the store. \
Returns exit code 0 if the content of the package is the same as it was at the time of unpacking',
            name: 'status',
          },
          {
            description: 'Adds new packages to the store. Example: pnpm store add express@4 typescript@2.1.0',
            name: 'add <pkg>...',
          },
          {
            description: '\
Removes unreferenced (extraneous, orphan) packages from the store. \
Pruning the store is not harmful, but might slow down future installations. \
Visit the documentation for more information on unreferenced packages and why they occur',
            name: 'prune',
          },
        ],
      },
    ],
    url: docsUrl('store'),
    usages: ['pnpm store <command>'],
  })
}

class StoreStatusError extends PnpmError {
  public modified: string[]
  constructor (modified: string[]) {
    super('MODIFIED_DEPENDENCY', '')
    this.modified = modified
  }
}

export type StoreCommandOptions = Pick<Config, 'dir' | 'registries' | 'tag' | 'storeDir'> & CreateStoreControllerOptions & {
  reporter?: (logObj: LogBase) => void
}

export async function handler (opts: StoreCommandOptions, params: string[]) {
  let store
  switch (params[0]) {
  case 'status':
    return statusCmd(opts)
  case 'prune': {
    store = await createOrConnectStoreController(opts)
    const storePruneOptions = Object.assign(opts, {
      storeController: store.ctrl,
      storeDir: store.dir,
    })
    return storePrune(storePruneOptions)
  }
  case 'add':
    store = await createOrConnectStoreController(opts)
    return storeAdd(params.slice(1), {
      prefix: opts.dir,
      registries: opts.registries,
      reporter: opts.reporter,
      storeController: store.ctrl,
      tag: opts.tag,
    })
  default:
    return help()
  }
}

async function statusCmd (opts: StoreCommandOptions) {
  const modifiedPkgs = await storeStatus(Object.assign(opts, {
    storeDir: await storePath(opts.dir, opts.storeDir),
  }))
  if (!modifiedPkgs || !modifiedPkgs.length) {
    logger.info({
      message: 'Packages in the store are untouched',
      prefix: opts.dir,
    })
    return
  }

  throw new StoreStatusError(modifiedPkgs)
}
