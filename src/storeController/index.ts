import {FetchFunction} from '@pnpm/fetcher-base'
import lock from '@pnpm/fs-locker'
import logger from '@pnpm/logger'
import createPackageRequester, {
  RequestPackageFunction,
} from '@pnpm/package-requester'
import {ResolveFunction} from '@pnpm/resolver-base'
import {StoreIndex} from '@pnpm/types'
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import rimraf = require('rimraf-then')
import {
  read as readStore,
  save as saveStore,
} from '../fs/storeIndex'
import createImportPackage, {copyPkg, ImportPackageFunction} from './createImportPackage'

export interface StoreController {
  requestPackage: RequestPackageFunction,
  importPackage: ImportPackageFunction,
  close (): Promise<void>,
  updateConnections (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}): Promise<void>,
  prune (): Promise<void>,
  saveState (): Promise<void>,
  upload (builtPkgLocation: string, opts: {pkgId: string, engine: string}): Promise<void>,
}

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
): Promise<StoreController> {
  const unlock = initOpts.locks
    ? await lock(initOpts.store, {
        locks: initOpts.locks,
        stale: initOpts.lockStaleDuration || 60 * 1000, // 1 minute,
      })
    : () => Promise.resolve(undefined)

  const store = initOpts.store
  const storeIndex = await readStore(initOpts.store) || {}
  const requestPackage = createPackageRequester(resolve, fetchers, {
    networkConcurrency: initOpts.networkConcurrency,
    storeIndex,
    storePath: initOpts.store,
  })

  return {
    close: async () => { await unlock() },
    importPackage: createImportPackage(initOpts.packageImportMethod),
    prune,
    requestPackage,
    saveState,
    updateConnections: async (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}) => {
      await removeDependencies(prefix, opts.removeDependencies, {prune: opts.prune})
      await addDependencies(prefix, opts.addDependencies)
    },
    upload,
  }

  function saveState () {
    return saveStore(initOpts.store, storeIndex)
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
          logger.info(`- ${pkgId}`)
        }
      }
    }
  }

  async function upload (builtPkgLocation: string, opts: {pkgId: string, engine: string}) {
    const cachePath = path.join(store, opts.pkgId, 'side_effects', opts.engine, 'package')
    // TODO calculate integrity.json here
    const filenames: string[] = []
    await copyPkg(builtPkgLocation, cachePath, {filesResponse: { fromStore: true, filenames }, force: true})
  }
}

const limitExistsCheck = pLimit(10)

async function getRemovedProject (storeIndex: StoreIndex) {
  const allProjects = R.uniq(R.unnest<string>(R.values(storeIndex)))

  return await pFilter(allProjects,
    (projectPath: string) => limitExistsCheck(async () => {
      const modulesDir = path.join(projectPath, 'node_modules')
      return !await exists(modulesDir)
    }))
}
