import { getFilePathInCafs as _getFilePathInCafs } from '@pnpm/cafs'
import { FetchFunction } from '@pnpm/fetcher-base'
import lock from '@pnpm/fs-locker'
import { globalInfo, globalWarn } from '@pnpm/logger'
import createPackageRequester, { getCacheByEngine } from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  ImportPackageFunction,
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
import { promisify } from 'util'
import writeJsonFile = require('write-json-file')
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
    ignoreFile?: (filename: string) => boolean,
    locks?: string,
    lockStaleDuration?: number,
    storeDir: string,
    networkConcurrency?: number,
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone',
    verifyStoreIntegrity: boolean,
  },
): Promise<StoreController & { closeSync: () => void, saveStateSync: () => void }> {
  const storeDir = initOpts.storeDir
  const unlock = initOpts.locks
    ? await lock(initOpts.storeDir, {
      locks: initOpts.locks,
      stale: initOpts.lockStaleDuration || 60 * 1000, // 1 minute,
      whenLocked: () => globalWarn(`waiting for the store at "${initOpts.storeDir}" to be unlocked...`),
    })
    : null

  const storeIndex = await readStore(initOpts.storeDir) || {}
  const packageRequester = createPackageRequester(resolve, fetchers, {
    ignoreFile: initOpts.ignoreFile,
    networkConcurrency: initOpts.networkConcurrency,
    storeDir: initOpts.storeDir,
    storeIndex,
    verifyStoreIntegrity: initOpts.verifyStoreIntegrity,
  })

  const impPkg = createImportPackage(initOpts.packageImportMethod)
  const cafsDir = path.join(storeDir, 'files')
  const getFilePathInCafs = _getFilePathInCafs.bind(null, cafsDir)
  const importPackage: ImportPackageFunction = (to, opts) => {
    const filesMap = {} as Record<string, string>
    for (const [fileName, fileMeta] of Object.entries(opts.filesResponse.filesIndex)) {
      filesMap[fileName] = getFilePathInCafs(fileMeta)
    }
    return impPkg(to, { filesMap, fromStore: opts.filesResponse.fromStore, force: opts.force })
  }

  return {
    close: unlock ? async () => { await unlock() } : () => Promise.resolve(undefined),
    closeSync: unlock ? () => unlock.sync() : () => undefined,
    fetchPackage: packageRequester.fetchPackageToStore,
    findPackageUsages,
    getPackageLocation,
    importPackage,
    prune,
    requestPackage: packageRequester.requestPackage,
    saveState: saveStore.bind(null, initOpts.storeDir, storeIndex),
    saveStateSync: saveStoreSync.bind(null, initOpts.storeDir, storeIndex),
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
      lockfileDir: string,
      targetEngine?: string,
    },
  ) {
    if (opts.targetEngine) {
      const sideEffectsCacheLocation = (await getCacheByEngine(initOpts.storeDir, packageId))[opts.targetEngine]
      if (sideEffectsCacheLocation) {
        return {
          dir: sideEffectsCacheLocation,
          isBuilt: true,
        }
      }
    }

    return {
      dir: path.join(initOpts.storeDir, pkgIdToFilename(packageId, opts.lockfileDir), 'node_modules', packageName),
      isBuilt: false,
    }
  }

  async function removeDependencies (prefix: string, dependencyPkgIds: string[], opts: {prune: boolean}) {
    await Promise.all(dependencyPkgIds.map(async (notDependent) => {
      if (storeIndex[notDependent]) {
        storeIndex[notDependent].splice(storeIndex[notDependent].indexOf(prefix), 1)
        if (opts.prune && !storeIndex[notDependent].length) {
          delete storeIndex[notDependent]
          await rimraf(path.join(storeDir, notDependent))
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
          await rimraf(path.join(storeDir, pkgId))
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
    const filesIndex = await packageRequester.cafs.addFilesFromDir(builtPkgLocation)
    // TODO: move this to a function
    // This is duplicated in @pnpm/package-requester
    const integrity = {}
    await Promise.all(
      Object.keys(filesIndex)
        .map(async (filename) => {
          const fileIntegrity = await filesIndex[filename].generatingIntegrity
          integrity[filename] = {
            integrity: fileIntegrity.toString(), // TODO: use the raw Integrity object
            mode: filesIndex[filename].mode,
            size: filesIndex[filename].size,
          }
        }),
    )
    const cachePath = path.join(storeDir, opts.packageId, 'side_effects', opts.engine)
    await writeJsonFile(path.join(cachePath, 'integrity.json'), integrity, { indent: undefined })
  }
}

const limitExistsCheck = pLimit(10)

async function getRemovedProject (storeIndex: StoreIndex) {
  const allProjects = R.uniq(R.unnest(Object.values(storeIndex)))

  return pFilter(allProjects,
    (projectPath: string) => limitExistsCheck(async () => {
      const modulesDir = path.join(projectPath, 'node_modules')
      return !await exists(modulesDir)
    }))
}
