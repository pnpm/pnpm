import logger, { storeLogger } from '@pnpm/logger'
import { FindPackageUsagesEntry, FindPackageUsagesResponse } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import archy = require('archy')
import {
  storeAdd,
  storePrune,
  storeStatus,
  storeUsages
} from 'supi'
import createStoreController from '../createStoreController'
import { PnpmError } from '../errorTypes'
import { PnpmOptions } from '../types'
import help from './help'

class StoreStatusError extends PnpmError {
  public modified: string[]
  constructor (modified: string[]) {
    super('MODIFIED_DEPENDENCY', '')
    this.modified = modified
  }
}

export default async function (input: string[], opts: PnpmOptions) {
  let store
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
      store = await createStoreController(opts)
      return storeAdd(input.slice(1), {
        prefix: opts.prefix,
        registry: opts.registry,
        reporter: opts.reporter,
        storeController: store.ctrl,
        tag: opts.tag,
        verifyStoreIntegrity: opts.verifyStoreIntegrity,
      })
    case 'usages':
      store = await createStoreController(opts)
      const packageUsages: FindPackageUsagesResponse[] = await storeUsages(input.slice(1), {
        reporter: opts.reporter,
        storeController: store.ctrl,
        tag: opts.tag,
      })
      prettyPrintUsages(packageUsages)
      return
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

/**
 * Uses archy to output package usages in a directory-tree like format.
 * @param packageUsagesResponses a list of package usages, one per query
 */
function prettyPrintUsages (packageUsagesResponses: FindPackageUsagesResponse[]): void {

  // Create nodes for top level usage response
  const packageUsageNodes: archy.Data[] = packageUsagesResponses.map(packageUsage => {
    if (!packageUsage.dependency) {
      storeLogger.error(new Error(`Internal error finding usages for ${JSON.stringify(packageUsage)}`))
      return {
        label: 'Internal error finding packages',
        nodes: []
      } as archy.Data
    }

    // Create label for root node
    const name: string | undefined = packageUsage.dependency.alias
    const tag: string | undefined = packageUsage.dependency.pref
    const label = name ?
      'Query: ' + name + (tag === 'latest' ? ' (any version)' : '@' + tag)
      : tag

    if (!packageUsage.foundInStore) {
      // If not found in store, just output string
      return {
        label,
        nodes: [
          'Not found in store'
        ]
      } as archy.Data
    }

    // This package was found in the store, create children for all package ids
    const foundPackages: FindPackageUsagesEntry[] = packageUsage.packages
    const foundPackagesNodes: archy.Data[] = foundPackages.map(foundPackage => {
      const label = 'Package in store: ' + foundPackage.id

      // Now create children for all locations this package is used
      const locations: string[] = foundPackage.usages
      const locationNodes: archy.Data[] = locations.map(location => {
        return {
          label: 'Project with dependency: ' + location
        } as archy.Data
      })

      // Now create node for the package found in the store
      return {
        label,
        nodes: locationNodes.length === 0 ? ['No pnpm projects using this package'] : locationNodes
      } as archy.Data
    })

    // Now create node for the original query
    return {
      label,
      nodes: foundPackagesNodes
    } as archy.Data
  })

  const rootTrees = packageUsageNodes.map(node => archy(node))
  rootTrees.forEach(tree => storeLogger.info(tree))
}
