import { FetchFunction } from '@pnpm/fetcher-base'
import lock from '@pnpm/fs-locker'
import { globalInfo, globalWarn } from '@pnpm/logger'
import createPackageRequester, { getCacheByEngine } from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  PackageUsagesBySearchQueries,
  StoreController,
} from '@pnpm/store-controller-types'
import { StoreIndex } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import pFilter = require('p-filter')
import pLimit from 'p-limit'
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import {
  read as readStore,
  save as saveStore,
  saveSync as saveStoreSync,
} from '../fs/storeIndex'
import createImportPackage, { copyPkg } from './createImportPackage'

export default async function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  initOpts: {
    locks?: string,
    lockStaleDuration?: number,
    store: string,
    networkConcurrency?: number,
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone',
    verifyStoreIntegrity: boolean,
  },
): Promise<StoreController & { closeSync: () => void, saveStateSync: () => void }> {
  const store = initOpts.store
  const unlock = initOpts.locks
    ? await lock(initOpts.store, {
      locks: initOpts.locks,
      stale: initOpts.lockStaleDuration || 60 * 1000, // 1 minute,
      whenLocked: () => globalWarn(`waiting for the store at "${initOpts.store}" to be unlocked...`),
    })
    : null

  const storeIndex = await readStore(initOpts.store) || {}
  const packageRequester = createPackageRequester(resolve, fetchers, {
    networkConcurrency: initOpts.networkConcurrency,
    storeIndex,
    storePath: initOpts.store,
    verifyStoreIntegrity: initOpts.verifyStoreIntegrity,
  })

  return {
    close: unlock ? async () => { await unlock() } : () => Promise.resolve(undefined),
    closeSync: unlock ? () => unlock.sync() : () => undefined,
    fetchPackage: packageRequester.fetchPackageToStore,
    findPackageUsages,
    getPackageLocation,
    importPackage: createImportPackage(initOpts.packageImportMethod),
    prune,
    requestPackage: packageRequester.requestPackage,
    saveState: saveStore.bind(null, initOpts.store, storeIndex),
    saveStateSync: saveStoreSync.bind(null, initOpts.store, storeIndex),
    updateConnections: async (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}) => {
      await removeDependencies(prefix, opts.removeDependencies, { prune: opts.prune })
      await addDependencies(prefix, opts.addDependencies)
    },
    upload,
  }

  async function getPackageLocation (
    packageId: string,
    packageName: string,
    opts: {
      lockfileDirectory: string,
      targetEngine?: string,
    }
  ) {
    if (opts.targetEngine) {
      const sideEffectsCacheLocation = (await getCacheByEngine(initOpts.store, packageId))[opts.targetEngine]
      if (sideEffectsCacheLocation) {
        return {
          directory: sideEffectsCacheLocation,
          isBuilt: true,
        }
      }
    }

    return {
      directory: path.join(initOpts.store, pkgIdToFilename(packageId, opts.lockfileDirectory), 'node_modules', packageName),
      isBuilt: false,
    }
  }

  async function removeDependencies (prefix: string, dependencyPkgIds: string[], opts: {prune: boolean}) {
    await Promise.all(dependencyPkgIds.map(async (notDependent) => {
      if (storeIndex[notDependent]) {
        storeIndex[notDependent].splice(storeIndex[notDependent].indexOf(prefix), 1)
        if (opts.prune && !storeIndex[notDependent].length) {
          delete storeIndex[notDependent]
          await rimraf(path.join(store, notDependent))
        }
      }
    }))
  }

  async function addDependencies (prefix: string, dependencyPkgIds: string[]) {
    dependencyPkgIds.forEach((newDependent) => {
      storeIndex[newDependent] = storeIndex[newDependent] || []
      if (!storeIndex[newDependent].includes(prefix)) {
        storeIndex[newDependent].push(prefix)
      }
    })
  }

  async function prune () {
    const removedProjects = await getRemovedProject(storeIndex)
    for (const pkgId in storeIndex) {
      if (storeIndex.hasOwnProperty(pkgId)) {
        storeIndex[pkgId] = R.difference(storeIndex[pkgId], removedProjects)

        if (!storeIndex[pkgId].length) {
          delete storeIndex[pkgId]
          await rimraf(path.join(store, pkgId))
          globalInfo(`- ${pkgId}`)
        }
      }
    }
  }

  async function findPackageUsages (searchQueries: string[]): Promise<PackageUsagesBySearchQueries> {
    const results = {} as PackageUsagesBySearchQueries

    // FIXME Inefficient looping over all packages. Don't think there's a better way.
    // Note we can't directly resolve packages because user may not specify package version
    Object.keys(storeIndex).forEach(packageId => {
      searchQueries
        .filter((searchQuery) => packageId.indexOf(searchQuery) > -1)
        .forEach((searchQuery) => {
          results[searchQuery] = results[searchQuery] || []
          results[searchQuery].push({
            packageId,
            usages: storeIndex[packageId] as string[],
          })
        })
    })

    return results
  }

  async function upload (builtPkgLocation: string, opts: {packageId: string, engine: string}) {
    const cachePath = path.join(store, opts.packageId, 'side_effects', opts.engine, 'package')
    // TODO calculate integrity.json here
    const filenames: string[] = []
    await copyPkg(builtPkgLocation, cachePath, { filesResponse: { fromStore: true, filenames }, force: true })
  }
}

const limitExistsCheck = pLimit(10)

async function getRemovedProject (storeIndex: StoreIndex) {
  const allProjects = R.uniq(R.unnest<string>(R.values(storeIndex)))

  return pFilter(allProjects,
    (projectPath: string) => limitExistsCheck(async () => {
      const modulesDir = path.join(projectPath, 'node_modules')
      return !await exists(modulesDir)
    }))
}
