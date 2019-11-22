import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import logger, { globalInfo } from '@pnpm/logger'
import { PackageUsages } from '@pnpm/store-controller-types'
import storePath from '@pnpm/store-path'
import archy = require('archy')
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  storeAdd,
  storePrune,
  storeStatus,
  storeUsages
} from 'supi'
import createStoreController from '../createStoreController'
import { PnpmOptions } from '../types'

export function types () {
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
            description: oneLine`
              Checks for modified packages in the store.
              Returns exit code 0 if the content of the package is the same as it was at the time of unpacking
            `,
            name: 'status',
          },
          {
            description: 'Adds new packages to the store. Example: pnpm store add express@4 typescript@2.1.0',
            name: 'add <pkg>...',
          },
          {
            description: 'Lists all pnpm projects on the current filesystem that depend on the specified packages. Example: pnpm store usages flatmap-stream',
            name: 'usages <pkg>...',
          },
          {
            description: oneLine`
              Removes unreferenced (extraneous, orphan) packages from the store.
              Pruning the store is not harmful, but might slow down future installations.
              Visit the documentation for more information on unreferenced packages and why they occur
            `,
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

export async function handler (input: string[], opts: PnpmOptions) {
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
      return help()
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
