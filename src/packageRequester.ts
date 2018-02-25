import checkPackage from '@pnpm/check-package'
import {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  DirectoryResolution,
  Resolution,
  ResolveFunction,
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolver-base'
import {
  PackageJson,
  PackageManifest,
  StoreIndex,
} from '@pnpm/types'
import {Stats} from 'fs'
import loadJsonFile = require('load-json-file')
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import PQueue = require('p-queue')
import path = require('path')
import exists = require('path-exists')
import renameOverwrite = require('rename-overwrite')
import rimraf = require('rimraf-then')
import symlinkDir = require('symlink-dir')
import writeJsonFile = require('write-json-file')
import {fromDir as readPkgFromDir} from './fs/readPkg'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {LoggedPkg, progressLogger} from './loggers'
import memoize, {MemoizedFunc} from './memoize'

export interface PackageFilesResponse {
  fromStore: boolean,
  filenames: string[],
}

export type PackageResponse = {
  body: {
    isLocal: true,
    resolution: DirectoryResolution,
    manifest: PackageManifest
    id: string,
    normalizedPref?: string,
  },
} | (
  {
    fetchingFiles?: Promise<PackageFilesResponse>,
    finishing?: Promise<void>, // a package request is finished once its integrity is generated and saved
    body: {
      isLocal: false,
      inStoreLocation: string,
      cacheByEngine: Map<string, string>,
      id: string,
      resolution: Resolution,
      // This is useful for recommending updates.
      // If latest does not equal the version of the
      // resolved package, it is out-of-date.
      latest?: string,
      normalizedPref?: string,
    },
  } & (
    {
      fetchingManifest: Promise<PackageManifest>,
    } | {
      body: {
        manifest: PackageManifest,
      },
    }
  )
)

export interface WantedDependency {
  alias?: string,
  pref: string,
}

export interface RequestPackageOptions {
  defaultTag?: string,
  skipFetch?: boolean,
  downloadPriority: number,
  loggedPkg: LoggedPkg,
  currentPkgId?: string,
  prefix: string,
  registry: string,
  shrinkwrapResolution?: Resolution,
  update?: boolean,
  verifyStoreIntegrity: boolean,
  preferredVersions: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
  sideEffectsCache: boolean,
}

export type RequestPackageFunction = (
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
) => Promise<PackageResponse>

export default function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  opts: {
    networkConcurrency?: number,
    storePath: string,
    storeIndex: StoreIndex,
  },
): RequestPackageFunction {
  opts = opts || {}

  const networkConcurrency = opts.networkConcurrency || 16
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })
  requestsQueue['counter'] = 0 // tslint:disable-line
  requestsQueue['concurrency'] = networkConcurrency // tslint:disable-line

  const fetch = fetcher.bind(null, fetchers)

  return resolveAndFetch.bind(null, {
    fetch,
    fetchingLocker: {},
    requestsQueue,
    resolve,
    storeIndex: opts.storeIndex,
    storePath: opts.storePath,
  })
}

async function resolveAndFetch (
  ctx: {
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    resolve: ResolveFunction,
    fetch: FetchFunction,
    fetchingLocker: {
      [pkgId: string]: {
        finishing: Promise<void>,
        fetchingFiles: Promise<PackageFilesResponse>,
        fetchingManifest?: Promise<PackageManifest>,
      },
    },
    storePath: string,
    storeIndex: StoreIndex,
  },
  wantedDependency: {
    alias?: string,
    pref: string,
  },
  options: {
    defaultTag?: string,
    downloadPriority: number,
    loggedPkg: LoggedPkg,
    currentPkgId?: string,
    prefix: string,
    registry: string,
    shrinkwrapResolution?: Resolution,
    update?: boolean,
    verifyStoreIntegrity: boolean,
    preferredVersions: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
    skipFetch: boolean,
    sideEffectsCache: boolean,
  },
): Promise<PackageResponse> {
  try {
    let latest: string | undefined
    let pkg: PackageJson | undefined
    let normalizedPref: string | undefined
    let resolution = options.shrinkwrapResolution as Resolution
    let pkgId = options.currentPkgId
    const skipResolution = resolution && !options.update
    let forceFetch = false

    // When fetching is skipped, resolution cannot be skipped.
    // We need the package's manifest when doing `shrinkwrap-only` installs.
    // When we don't fetch, the only way to get the package's manifest is via resolving it.
    //
    // The resolution step is never skipped for local dependencies.
    if (!skipResolution || options.skipFetch || pkgId && pkgId.startsWith('file:')) {
      const resolveResult = await ctx.requestsQueue.add<ResolveResult>(() => ctx.resolve(wantedDependency, {
        defaultTag: options.defaultTag,
        preferredVersions: options.preferredVersions,
        prefix: options.prefix,
        registry: options.registry,
      }), {priority: options.downloadPriority})

      pkg = resolveResult.package
      latest = resolveResult.latest

      // If the integrity of a local tarball dependency has changed,
      // the local tarball should be unpacked, so a fetch to the store should be forced
      forceFetch = Boolean(
        options.shrinkwrapResolution &&
        pkgId && pkgId.startsWith('file:') &&
        options.shrinkwrapResolution['integrity'] !== resolveResult.resolution['integrity'], // tslint:disable-line:no-string-literal
      )

      if (!skipResolution || forceFetch) {
        // Keep the shrinkwrap resolution when possible
        // to keep the original shasum.
        if (pkgId !== resolveResult.id || !resolution || forceFetch) {
          resolution = resolveResult.resolution
        }
        pkgId = resolveResult.id
        normalizedPref = resolveResult.normalizedPref
      }
    }

    const id = pkgId as string

    progressLogger.debug({status: 'resolved', pkgId: id, pkg: options.loggedPkg})

    if (resolution.type === 'directory') {
      if (!pkg) {
        throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref}`)
      }
      return {
        body: {
          cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
          id,
          isLocal: true,
          manifest: pkg,
          normalizedPref,
          resolution: resolution as DirectoryResolution,
        },
      }
    }

    const targetRelative = pkgIdToFilename(id)
    const target = path.join(ctx.storePath, targetRelative)

    // We can skip fetching the package only if the manifest
    // is present after resolution
    if (options.skipFetch && pkg) {
      return {
        body: {
          cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
          id,
          inStoreLocation: target,
          isLocal: false as false,
          latest,
          manifest: pkg,
          normalizedPref,
          resolution,
        },
      }
    }

    if (!ctx.fetchingLocker[id]) {
      ctx.fetchingLocker[id] = fetchToStore({
        fetch: ctx.fetch,
        forceFetch,
        pkg,
        pkgId: id,
        prefix: options.prefix,
        requestsQueue: ctx.requestsQueue,
        resolution: resolution as Resolution,
        storeIndex: ctx.storeIndex,
        storePath: ctx.storePath,
        target,
        targetRelative,
        verifyStoreIntegrity: options.verifyStoreIntegrity,
      })
    }

    if (pkg) {
      return {
        body: {
          cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
          id,
          inStoreLocation: target,
          isLocal: false as false,
          latest,
          manifest: pkg,
          normalizedPref,
          resolution,
        },
        fetchingFiles: ctx.fetchingLocker[id].fetchingFiles,
        finishing: ctx.fetchingLocker[id].finishing,
      }
    }
    return {
      body: {
        cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
        id,
        inStoreLocation: target,
        isLocal: false as false,
        latest,
        normalizedPref,
        resolution,
      },
      fetchingFiles: ctx.fetchingLocker[id].fetchingFiles,
      fetchingManifest: ctx.fetchingLocker[id].fetchingManifest as Promise<PackageManifest>,
      finishing: ctx.fetchingLocker[id].finishing,
    }
  } catch (err) {
    progressLogger.debug({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

function fetchToStore (opts: {
  fetch: FetchFunction,
  forceFetch: boolean,
  requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
  pkg?: PackageJson,
  pkgId: string,
  prefix: string,
  resolution: Resolution,
  target: string,
  targetRelative: string,
  storeIndex: StoreIndex,
  storePath: string,
  verifyStoreIntegrity: boolean,
}): {
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingManifest?: Promise<PackageManifest>,
  finishing: Promise<void>,
} {
  const fetchingManifest = differed<PackageManifest>()
  const fetchingFiles = differed<PackageFilesResponse>()
  const finishing = differed<void>()

  doFetchToStore()

  if (!opts.pkg) {
    return {
      fetchingFiles: fetchingFiles.promise,
      fetchingManifest: fetchingManifest.promise,
      finishing: finishing.promise,
    }
  }
  return {
    fetchingFiles: fetchingFiles.promise,
    finishing: finishing.promise,
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

      if (!opts.forceFetch && targetExists) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = opts.verifyStoreIntegrity
          ? await checkPackage(linkToUnpacked)
          : await loadJsonFile(path.join(path.dirname(linkToUnpacked), 'integrity.json'))
        if (satisfiedIntegrity) {
          progressLogger.debug({
            pkgId: opts.pkgId,
            status: 'found_in_store',
          })
          fetchingFiles.resolve({
            filenames: Object.keys(satisfiedIntegrity).filter((f) => !satisfiedIntegrity[f].isDir), // Filtering can be removed for store v3
            fromStore: true,
          })
          if (!opts.pkg) {
            readPkgFromDir(linkToUnpacked)
              .then(fetchingManifest.resolve)
              .catch(fetchingManifest.reject)
          }
          finishing.resolve(undefined)
          return
        }
        logger.warn(`Refetching ${target} to store, as it was modified`)
      }

      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      let filesIndex!: {}
      let tempLocation!: string
      await Promise.all([
        (async () => {
          // Tarballs are requested first because they are bigger than metadata files.
          // However, when one line is left available, allow it to be picked up by a metadata request.
          // This is done in order to avoid situations when tarballs are downloaded in chunks
          // As much tarballs should be downloaded simultaneously as possible.
          const priority = (++opts.requestsQueue['counter'] % opts.requestsQueue['concurrency'] === 0 ? -1 : 1) * 1000 // tslint:disable-line

          const fetchedPackage = await opts.requestsQueue.add(() => opts.fetch(opts.resolution, target, {
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

          filesIndex = fetchedPackage.filesIndex
          tempLocation = fetchedPackage.tempLocation
        })(),
        // removing only the folder with the unpacked files
        // not touching tarball and integrity.json
        targetExists && await rimraf(path.join(target, 'node_modules')),
      ])
      progressLogger.debug({
        pkgId: opts.pkgId,
        status: 'fetched',
      })

      // Ideally, fetchingFiles wouldn't care about when integrity is calculated.
      // However, we can only rename the temp folder once we know the package name.
      // And we cannot rename the temp folder till we're calculating integrities.
      if (!targetExists) {
        if (opts.verifyStoreIntegrity) {
          const fileIntegrities = await Promise.all(
            Object.keys(filesIndex)
              .map((filename) =>
              filesIndex[filename].generatingIntegrity
                  .then((fileIntegrity: object) => ({
                    [filename]: {
                      integrity: fileIntegrity,
                      size: filesIndex[filename].size,
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
          await writeJsonFile(path.join(target, 'integrity.json'), filesIndex, {indent: null})
        }
        finishing.resolve(undefined)
      } else {
        finishing.resolve(undefined)
      }

      let pkg: PackageJson
      if (opts.pkg) {
        pkg = opts.pkg
      } else {
        pkg = await readPkgFromDir(tempLocation)
        fetchingManifest.resolve(pkg)
      }

      const unpacked = path.join(target, 'node_modules', pkg.name)
      await mkdirp(path.dirname(unpacked))

      // rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await renameOverwrite(tempLocation, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      fetchingFiles.resolve({
        filenames: Object.keys(filesIndex).filter((f) => !filesIndex[f].isDir), // Filtering can be removed for store v3
        fromStore: false,
      })
    } catch (err) {
      fetchingFiles.reject(err)
      if (!opts.pkg) {
        fetchingManifest.reject(err)
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
): Promise<FetchResult> {
  const fetch = fetcherByHostingType[resolution.type || 'tarball']
  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type}" is not supported`)
  }
  try {
    return await fetch(resolution, target, opts)
  } catch (err) {
    logger.error(`Fetching ${opts.pkgId} failed!`)
    throw err
  }
}

async function getCacheByEngine (storePath: string, id: string): Promise<Map<string, string>> {
  const map = new Map()

  const cacheRoot = path.join(storePath, id, 'side_effects')
  if (!await fs.exists(cacheRoot)) {
    return map
  }

  const dirContents = (await fs.readdir(cacheRoot)).map((content) => path.join(cacheRoot, content))
  await Promise.all(dirContents.map(async (dir) => {
    if (!(await fs.lstat(dir)).isDirectory()) {
      return
    }
    const engineName = path.basename(dir)
    map[engineName] = path.join(dir, 'package')
  }))

  return map
}
