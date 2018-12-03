import { FetchFunction } from '@pnpm/fetcher-base'
import lock from '@pnpm/fs-locker'
import { storeLogger } from '@pnpm/logger'
import createPackageRequester, { getCacheByEngine } from '@pnpm/package-requester'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  FindPackageUsagesEntry,
  FindPackageUsagesResponse,
  StoreController,
  WantedDependency
} from '@pnpm/store-controller-types'
import { StoreIndex } from '@pnpm/types'
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import rimraf = require('rimraf-then')
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
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink',
  },
): Promise<StoreController & { closeSync: () => void, saveStateSync: () => void }> {
  const unlock = initOpts.locks
    ? await lock(initOpts.store, {
      locks: initOpts.locks,
      stale: initOpts.lockStaleDuration || 60 * 1000, // 1 minute,
      whenLocked: () => storeLogger.warn(`waiting for the store at "${initOpts.store}" to be unlocked...`),
    })
    : null

  const store = initOpts.store
  const storeIndex = await readStore(initOpts.store) || {}
  const packageRequester = createPackageRequester(resolve, fetchers, {
    networkConcurrency: initOpts.networkConcurrency,
    storeIndex,
    storePath: initOpts.store,
  })

  return {
    close: unlock ? async () => { await unlock() } : () => Promise.resolve(undefined),
    closeSync: unlock ? () => unlock.sync() : () => undefined,
    fetchPackage: packageRequester.fetchPackageToStore,
    findPackageUsages,
    getCacheByEngine,
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
      if (storeIndex[newDependent].indexOf(prefix) === -1) {
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
          storeLogger.info(`- ${pkgId}`)
        }
      }
    }
  }

  async function findPackageUsages (
    dependencies: WantedDependency[]
  ): Promise<FindPackageUsagesResponse[]> {

    /**
     * If `dependency` has no `alias`, then directly check against `pref`.
     * This is messy due to overloaded use WantedDependency (npm, github, tarball, etc.)
     * @param dependency the dependency to parse
     */
    function parseDependency (dependency: WantedDependency) {
      let packageName
      let packageTag
      if (dependency.alias) {
        packageName = `/${dependency.alias}/`
        packageTag = dependency.pref === 'latest' ? null : dependency.pref
      } else {
        packageName = dependency.alias
      }

      return {
        packageName,
        packageTag
      }
    }

    // Parse dependencies
    const parsedDeps = dependencies.map(dep => parseDependency(dep))

    // Create initial results map for each dependency
    const results = dependencies.map(dependency => {
      return {
        dependency,
        foundInStore: false,
        packages: [] as FindPackageUsagesEntry[]
      }
    })

    // FIXME Inefficient looping over all packages. Don't think there's a better way.
    // Note we can't directly resolve packages because user may not specify package version
    Object.keys(storeIndex).forEach(packageId => {
      parsedDeps.forEach((parsedDep, index) => {

        // Check if package name and tag (if specified) match this package id
        if (parsedDep.packageName && packageId.indexOf(parsedDep.packageName) > -1
            && (!parsedDep.packageTag || packageId.indexOf(parsedDep.packageTag) > -1)) {

          // Found match!
          const packageEntry = {
            id: packageId,
            usages: storeIndex[packageId] as string[]
          }

          // Update results with new entry
          results[index].packages.push(packageEntry)
          results[index].foundInStore = true
        }

      })
    })

    return results as FindPackageUsagesResponse[]
  }

  async function upload (builtPkgLocation: string, opts: {pkgId: string, engine: string}) {
    const cachePath = path.join(store, opts.pkgId, 'side_effects', opts.engine, 'package')
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
