import PnpmError from '@pnpm/error'
import logger, { globalInfo } from '@pnpm/logger'
import { PackageUsages } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import archy = require('archy')
import {
  storeAdd,
  storePrune,
  storeStatus,
  storeUsages
} from 'supi'
import createStoreController from '../createStoreController'
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
        storeController: store.ctrl,
        storeDir: store.dir,
      })
      return storePrune(storePruneOptions)
    case 'add':
      store = await createStoreController(opts)
      return storeAdd(input.slice(1), {
        prefix: opts.dir,
        registries: opts.registries,
        reporter: opts.reporter,
        storeController: store.ctrl,
        tag: opts.tag,
      })
    case 'usages':
      store = await createStoreController(opts)
      const packageSelectors = input.slice(1)
      const packageUsagesBySelectors = await storeUsages(packageSelectors, {
        reporter: opts.reporter,
        storeController: store.ctrl,
      })
      prettyPrintUsages(packageSelectors, packageUsagesBySelectors)
      return
    default:
      help(['store'])
      if (input[0]) {
        throw new PnpmError('INVALID_STORE_COMMAND', `"store ${input[0]}" is not a pnpm command. See "pnpm help store".`)
      }
  }
}

async function statusCmd (opts: PnpmOptions) {
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

/**
 * Uses archy to output package usages in a directory-tree like format.
 * @param packageUsages a list of PackageUsage, one per query
 */
function prettyPrintUsages (selectors: string[], packageUsagesBySelectors: { [packageSelector: string]: PackageUsages[] }): void {

  // Create nodes for top level usage response
  const packageUsageNodes: archy.Data[] = selectors.map((selector) => {
    // Create label for root node
    const label = `Package: ${selector}`

    if (!packageUsagesBySelectors[selector].length) {
      // If not found in store, just output string
      return {
        label,
        nodes: [
          'Not found in store'
        ]
      } as archy.Data
    }

    // This package was found in the store, create children for all package ids
    const foundPackagesNodes: archy.Data[] = packageUsagesBySelectors[selector].map((foundPackage) => {
      const label = `Package in store: ${foundPackage.packageId}`

      // Now create children for all locations this package id is used
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
  rootTrees.forEach(tree => globalInfo(tree))
}
