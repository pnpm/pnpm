import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import {Stats} from 'fs'
import loadJsonFile = require('load-json-file')
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import PQueue = require('p-queue')
import {
  pkgIdToFilename,
  pkgIsUntouched,
  Store,
} from 'package-store'
import path = require('path')
import exists = require('path-exists')
import renameOverwrite = require('rename-overwrite')
import rimraf = require('rimraf-then')
import symlinkDir = require('symlink-dir')
import * as unpackStream from 'unpack-stream'
import writeJsonFile = require('write-json-file')
import {
  FetchFunction,
  FetchOptions,
} from './fetchTypes'
import {fromDir as readPkgFromDir} from './fs/readPkg'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {LoggedPkg, progressLogger} from './loggers'
import memoize, {MemoizedFunc} from './memoize'
import {
  DirectoryResolution,
  Resolution,
  ResolveFunction,
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from './resolveTypes'

export interface PackageContentInfo {
  isNew: boolean,
  index: {},
}

export type FetchedPackage = {
  isLocal: true,
  resolution: DirectoryResolution,
  pkg: PackageJson,
  id: string,
  normalizedPref?: string,
} | {
  isLocal: false,
  fetchingPkg: Promise<PackageJson>,
  fetchingFiles: Promise<PackageContentInfo>,
  calculatingIntegrity: Promise<void>,
  path: string,
  id: string,
  resolution: Resolution,
  // This is useful for recommending updates.
  // If latest does not equal the version of the
  // resolved package, it is out-of-date.
  latest?: string,
  normalizedPref?: string,
}

export default function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  opts: {
    networkConcurrency: number,
  },
) {
  opts = opts || {}

  const networkConcurrency = opts.networkConcurrency || 16
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })
  requestsQueue['counter'] = 0 // tslint:disable-line
  requestsQueue['concurrency'] = networkConcurrency // tslint:disable-line

  const fetch = fetcher.bind(null, fetchers)

  return resolveAndFetch.bind(null,
    requestsQueue,
    resolve,
    fetch,
  )
}

async function resolveAndFetch (
  requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
  resolve: ResolveFunction,
  fetch: FetchFunction,
  wantedDependency: {
    alias?: string,
    pref: string,
  },
  options: {
    downloadPriority: number,
    fetchingLocker: {
      [pkgId: string]: {
        calculatingIntegrity: Promise<void>,
        fetchingFiles: Promise<PackageContentInfo>,
        fetchingPkg: Promise<PackageJson>,
      },
    },
    loggedPkg: LoggedPkg,
    offline: boolean,
    pkgId?: string,
    prefix: string,
    registry: string,
    shrinkwrapResolution?: Resolution,
    storeIndex: Store,
    storePath: string,
    update?: boolean,
    verifyStoreIntegrity: boolean,
  },
): Promise<FetchedPackage> {
  try {
    let latest: string | undefined
    let pkg: PackageJson | undefined
    let normalizedPref: string | undefined
    let resolution = options.shrinkwrapResolution
    let pkgId = options.pkgId
    if (!resolution || options.update) {
      const resolveResult = await requestsQueue.add<ResolveResult>(() => resolve(wantedDependency, {
        prefix: options.prefix,
        registry: options.registry,
      }), {priority: options.downloadPriority})
      // keep the shrinkwrap resolution when possible
      // to keep the original shasum
      if (pkgId !== resolveResult.id || !resolution) {
        resolution = resolveResult.resolution
      }
      pkgId = resolveResult.id
      pkg = resolveResult.package
      latest = resolveResult.latest
      normalizedPref = resolveResult.normalizedPref
    }

    const id = pkgId as string

    progressLogger.debug({status: 'resolved', pkgId: id, pkg: options.loggedPkg})

    if (resolution.type === 'directory') {
      if (!pkg) {
        throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref}`)
      }
      return {
        id,
        isLocal: true,
        normalizedPref,
        pkg,
        resolution: resolution as DirectoryResolution,
      }
    }

    const targetRelative = pkgIdToFilename(id)
    const target = path.join(options.storePath, targetRelative)

    if (!options.fetchingLocker[id]) {
      options.fetchingLocker[id] = fetchToStore({
        fetch,
        pkg,
        pkgId: id,
        prefix: options.prefix,
        requestsQueue,
        resolution: resolution as Resolution,
        storeIndex: options.storeIndex,
        storePath: options.storePath,
        target,
        targetRelative,
        verifyStoreIntegrity: options.verifyStoreIntegrity,
      })
    }

    return {
      calculatingIntegrity: options.fetchingLocker[id].calculatingIntegrity,
      fetchingFiles: options.fetchingLocker[id].fetchingFiles,
      fetchingPkg: options.fetchingLocker[id].fetchingPkg,
      id,
      isLocal: false,
      latest,
      normalizedPref,
      path: target,
      resolution,
    }
  } catch (err) {
    progressLogger.debug({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

function fetchToStore (opts: {
  fetch: FetchFunction,
  requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
  pkg?: PackageJson,
  pkgId: string,
  prefix: string,
  resolution: Resolution,
  target: string,
  targetRelative: string,
  storePath: string,
  storeIndex: Store,
  verifyStoreIntegrity: boolean,
}): {
  fetchingFiles: Promise<PackageContentInfo>,
  fetchingPkg: Promise<PackageJson>,
  calculatingIntegrity: Promise<void>,
} {
  const fetchingPkg = differed<PackageJson>()
  const fetchingFiles = differed<PackageContentInfo>()
  const calculatingIntegrity = differed<void>()

  doFetchToStore()

  return {
    calculatingIntegrity: calculatingIntegrity.promise,
    fetchingFiles: fetchingFiles.promise,
    fetchingPkg: opts.pkg && Promise.resolve(opts.pkg) || fetchingPkg.promise,
  }

  async function doFetchToStore () {
    try {
      progressLogger.debug({
        pkgId: opts.pkgId,
        status: 'resolving_content',
      })

      const target = opts.target
      const linkToUnpacked = path.join(target, 'package')

      // We can safely assume that if there is no data about the package in `store.json` then
      // it is not in the store yet.
      // In case there is record about the package in `store.json`, we check it in the file system just in case
      const targetExists = opts.storeIndex[opts.targetRelative] && await exists(path.join(linkToUnpacked, 'package.json'))

      if (targetExists) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = opts.verifyStoreIntegrity
          ? await pkgIsUntouched(linkToUnpacked)
          : await loadJsonFile(path.join(path.dirname(linkToUnpacked), 'integrity.json'))
        if (satisfiedIntegrity) {
          progressLogger.debug({
            pkgId: opts.pkgId,
            status: 'found_in_store',
          })
          fetchingFiles.resolve({
            index: satisfiedIntegrity,
            isNew: false,
          })
          if (!opts.pkg) {
            readPkgFromDir(linkToUnpacked)
              .then(fetchingPkg.resolve)
              .catch(fetchingPkg.reject)
          }
          calculatingIntegrity.resolve(undefined)
          return
        }
        logger.warn(`Refetching ${target} to store, as it was modified`)
      }

      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      const targetStage = `${target}_stage`

      await rimraf(targetStage)

      let packageIndex: {} = {}
      await Promise.all([
        (async () => {
          // Tarballs are requested first because they are bigger than metadata files.
          // However, when one line is left available, allow it to be picked up by a metadata request.
          // This is done in order to avoid situations when tarballs are downloaded in chunks
          // As much tarballs should be downloaded simultaneously as possible.
          const priority = (++opts.requestsQueue['counter'] % opts.requestsQueue['concurrency'] === 0 ? -1 : 1) * 1000 // tslint:disable-line

          packageIndex = await opts.requestsQueue.add(() => opts.fetch(opts.resolution, targetStage, {
            cachedTarballLocation: path.join(opts.storePath, opts.pkgId, 'packed.tgz'),
            onProgress: (downloaded) => {
              progressLogger.debug({status: 'fetching_progress', pkgId: opts.pkgId, downloaded})
            },
            onStart: (size, attempt) => {
              progressLogger.debug({status: 'fetching_started', pkgId: opts.pkgId, size, attempt})
            },
            pkgId: opts.pkgId,
            prefix: opts.prefix,
          }), {priority})
        })(),
        // removing only the folder with the unpacked files
        // not touching tarball and integrity.json
        targetExists && await rimraf(path.join(target, 'node_modules')),
      ])
      progressLogger.debug({
        pkgId: opts.pkgId,
        status: 'fetched',
      })

      // fetchingFilse shouldn't care about when this is saved at all
      if (!targetExists) {
        (async () => {
          if (opts.verifyStoreIntegrity) {
            const fileIntegrities = await Promise.all(
              Object.keys(packageIndex)
                .map((filename) =>
                  packageIndex[filename].generatingIntegrity
                    .then((fileIntegrity: object) => ({
                      [filename]: {
                        integrity: fileIntegrity,
                        size: packageIndex[filename].size,
                      },
                    })),
                ),
            )
            const integrity = fileIntegrities
              .reduce((acc, info) => {
                Object.assign(acc, info)
                return acc
              }, {})
            await writeJsonFile(path.join(target, 'integrity.json'), integrity, {indent: null})
          } else {
            // TODO: save only filename: {size}
            await writeJsonFile(path.join(target, 'integrity.json'), packageIndex, {indent: null})
          }
          calculatingIntegrity.resolve(undefined)
        })()
      } else {
        calculatingIntegrity.resolve(undefined)
      }

      let pkg: PackageJson
      if (opts.pkg) {
        pkg = opts.pkg
      } else {
        pkg = await readPkgFromDir(targetStage)
        fetchingPkg.resolve(pkg)
      }

      const unpacked = path.join(target, 'node_modules', pkg.name)
      await mkdirp(path.dirname(unpacked))

      // rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await renameOverwrite(targetStage, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      fetchingFiles.resolve({
        index: packageIndex,
        isNew: true,
      })
    } catch (err) {
      fetchingFiles.reject(err)
      if (!opts.pkg) {
        fetchingPkg.reject(err)
      }
    }
  }
}

// tslint:disable-next-line
function noop () {}

function differed<T> (): {
  promise: Promise<T>,
  resolve: (v: T) => void,
  reject: (err: Error) => void,
} {
  let pResolve: (v: T) => void = noop
  let pReject: (err: Error) => void = noop
  const promise = new Promise<T>((resolve, reject) => {
    pResolve = resolve
    pReject = reject
  })
  return {
    promise,
    reject: pReject,
    resolve: pResolve,
  }
}

async function fetcher (
  fetcherByHostingType: {[hostingType: string]: FetchFunction},
  resolution: Resolution,
  target: string,
  opts: FetchOptions,
): Promise<unpackStream.Index> {
  const fetch = fetcherByHostingType[resolution.type || 'tarball']
  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type}" is not supported`)
  }
  return await fetch(resolution, target, opts)
}
