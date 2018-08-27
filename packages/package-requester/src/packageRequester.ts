import checkPackage from '@pnpm/check-package'
import {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import {storeLogger} from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  DirectoryResolution,
  LocalPackages,
  Resolution,
  ResolveFunction,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolver-base'
import {
  PackageJson,
  PackageManifest,
  StoreIndex,
} from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import PQueue = require('p-queue')
import path = require('path')
import exists = require('path-exists')
import renameOverwrite = require('rename-overwrite')
import rimraf = require('rimraf-then')
import symlinkDir = require('symlink-dir')
import writeJsonFile from 'write-json-file'
import {fromDir as readPkgFromDir} from './fs/readPkg'
import {LoggedPkg, progressLogger} from './loggers'

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
    updated: boolean,
    resolvedVia?: string,
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
      updated: boolean,
      resolvedVia?: string,
    },
  } & (
    {
      fetchingRawManifest: Promise<PackageJson>,
    } | {
      body: {
        manifest: PackageManifest,
        updated: boolean,
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
  verifyStoreIntegrity: boolean, // TODO: this should be a context field
  preferredVersions: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
  localPackages?: LocalPackages,
  sideEffectsCache?: boolean,
}

export type RequestPackageFunction = (
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
) => Promise<PackageResponse>

export interface FetchPackageToStoreOptions {
  fetchRawManifest?: boolean,
  force: boolean,
  pkgName?: string,
  pkgId: string,
  prefix: string,
  resolution: Resolution,
  verifyStoreIntegrity: boolean, // TODO: this should be a context field
}

export type FetchPackageToStoreFunction = (
  opts: FetchPackageToStoreOptions,
) => {
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingRawManifest?: Promise<PackageJson>,
  finishing: Promise<void>,
  inStoreLocation: string,
}

export default function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  opts: {
    networkConcurrency?: number,
    storePath: string,
    storeIndex: StoreIndex,
  },
): RequestPackageFunction & {
  fetchPackageToStore: FetchPackageToStoreFunction,
  requestPackage: RequestPackageFunction,
} {
  opts = opts || {}

  const networkConcurrency = opts.networkConcurrency || 16
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })
  requestsQueue['counter'] = 0 // tslint:disable-line
  requestsQueue['concurrency'] = networkConcurrency // tslint:disable-line

  const fetch = fetcher.bind(null, fetchers)
  const fetchPackageToStore = fetchToStore.bind(null, {
    fetch,
    fetchingLocker: new Map(),
    requestsQueue,
    storeIndex: opts.storeIndex,
    storePath: opts.storePath,
  })
  const requestPackage = resolveAndFetch.bind(null, {
    fetchPackageToStore,
    requestsQueue,
    resolve,
    storePath: opts.storePath,
  })

  requestPackage['requestPackage'] = requestPackage // tslint:disable-line
  requestPackage['fetchPackageToStore'] = fetchPackageToStore // tslint:disable-line

  return requestPackage
}

async function resolveAndFetch (
  ctx: {
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    resolve: ResolveFunction,
    fetchPackageToStore: FetchPackageToStoreFunction,
    storePath: string,
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
    localPackages?: LocalPackages,
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
    let updated = false
    let resolvedVia: string | undefined

    // When fetching is skipped, resolution cannot be skipped.
    // We need the package's manifest when doing `shrinkwrap-only` installs.
    // When we don't fetch, the only way to get the package's manifest is via resolving it.
    //
    // The resolution step is never skipped for local dependencies.
    if (!skipResolution || options.skipFetch || pkgId && pkgId.startsWith('file:')) {
      const resolveResult = await ctx.requestsQueue.add<ResolveResult>(() => ctx.resolve(wantedDependency, {
        defaultTag: options.defaultTag,
        localPackages: options.localPackages,
        preferredVersions: options.preferredVersions,
        prefix: options.prefix,
        registry: options.registry,
      }), {priority: options.downloadPriority})

      pkg = resolveResult.package
      latest = resolveResult.latest
      resolvedVia = resolveResult.resolvedVia

      // If the integrity of a local tarball dependency has changed,
      // the local tarball should be unpacked, so a fetch to the store should be forced
      forceFetch = Boolean(
        options.shrinkwrapResolution &&
        pkgId && pkgId.startsWith('file:') &&
        options.shrinkwrapResolution['integrity'] !== resolveResult.resolution['integrity'], // tslint:disable-line:no-string-literal
      )

      if (!skipResolution || forceFetch) {
        updated = pkgId !== resolveResult.id || !resolution || forceFetch
        // Keep the shrinkwrap resolution when possible
        // to keep the original shasum.
        if (updated) {
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
          resolvedVia,
          updated,
        },
      }
    }

    // We can skip fetching the package only if the manifest
    // is present after resolution
    if (options.skipFetch && pkg) {
      return {
        body: {
          cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
          id,
          inStoreLocation: path.join(ctx.storePath, pkgIdToFilename(id)),
          isLocal: false as false,
          latest,
          manifest: pkg,
          normalizedPref,
          resolution,
          resolvedVia,
          updated,
        },
      }
    }

    const fetchResult = ctx.fetchPackageToStore({
      fetchRawManifest: updated || !pkg,
      force: forceFetch,
      pkgId: id,
      pkgName: pkg && pkg.name,
      prefix: options.prefix,
      resolution: resolution as Resolution,
      verifyStoreIntegrity: options.verifyStoreIntegrity,
    })

    return {
      body: {
        cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storePath, id) : new Map(),
        id,
        inStoreLocation: fetchResult.inStoreLocation,
        isLocal: false as false,
        latest,
        manifest: pkg,
        normalizedPref,
        resolution,
        resolvedVia,
        updated,
      },
      fetchingFiles: fetchResult.fetchingFiles,
      fetchingRawManifest: fetchResult.fetchingRawManifest,
      finishing: fetchResult.finishing,
    } as PackageResponse
  } catch (err) {
    progressLogger.debug({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

function fetchToStore (
  ctx: {
    fetch: FetchFunction,
    fetchingLocker: Map<string, {
      finishing: Promise<void>,
      fetchingFiles: Promise<PackageFilesResponse>,
      fetchingRawManifest?: Promise<PackageJson>,
      inStoreLocation: string,
    }>,
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    storeIndex: StoreIndex,
    storePath: string,
  },
  opts: {
    fetchRawManifest?: boolean,
    force: boolean,
    pkgName?: string,
    pkgId: string,
    prefix: string,
    resolution: Resolution,
    verifyStoreIntegrity: boolean,
  },
): {
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingRawManifest?: Promise<PackageJson>,
  finishing: Promise<void>,
  inStoreLocation: string,
} {
  const targetRelative = pkgIdToFilename(opts.pkgId)
  const target = path.join(ctx.storePath, targetRelative)

  if (!ctx.fetchingLocker.has(opts.pkgId)) {
    const fetchingRawManifest = differed<PackageJson>()
    const fetchingFiles = differed<PackageFilesResponse>()
    const finishing = differed<void>()

    doFetchToStore(fetchingRawManifest, fetchingFiles, finishing)

    if (opts.fetchRawManifest) {
      ctx.fetchingLocker.set(opts.pkgId, {
        fetchingFiles: removeKeyOnFail(fetchingFiles.promise),
        fetchingRawManifest: removeKeyOnFail(fetchingRawManifest.promise),
        finishing: removeKeyOnFail(finishing.promise),
        inStoreLocation: target,
      })
    } else {
      ctx.fetchingLocker.set(opts.pkgId, {
        fetchingFiles: removeKeyOnFail(fetchingFiles.promise),
        finishing: removeKeyOnFail(finishing.promise),
        inStoreLocation: target,
      })
    }

    // When fetchingFiles resolves, the cached result has to set fromStore to true, without
    // affecting previous invocations: so we need to replace the cache.
    //
    // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
    // Otherwise, if a package was not in store when the server started, it will be always
    // reported as "downloaded" instead of "reused".
    fetchingFiles.promise.then(({filenames, fromStore}) => {
      // If it's already in the store, we don't need to update the cache
      if (fromStore) {
        return
      }

      const tmp = ctx.fetchingLocker.get(opts.pkgId) as {
        fetchingFiles: Promise<PackageFilesResponse>,
        fetchingRawManifest?: Promise<PackageJson>,
        finishing: Promise<void>,
        inStoreLocation: string,
      }

      // If fetching failed then it was removed from the cache.
      // It is OK. In that case there is no need to update it.
      if (!tmp) return

      ctx.fetchingLocker.set(opts.pkgId, {
        fetchingFiles: Promise.resolve({
          filenames,
          fromStore: true,
        }),
        fetchingRawManifest: tmp.fetchingRawManifest,
        finishing: tmp.finishing,
        inStoreLocation: tmp.inStoreLocation,
      })
    })

    fetchingFiles.promise.catch((err) => {
      ctx.fetchingLocker.delete(opts.pkgId)
      throw err
    })
  }

  const result = ctx.fetchingLocker.get(opts.pkgId) as {
    fetchingFiles: Promise<PackageFilesResponse>,
    fetchingRawManifest?: Promise<PackageJson>,
    finishing: Promise<void>,
    inStoreLocation: string,
  }

  if (opts.fetchRawManifest && !result.fetchingRawManifest) {
    result.fetchingRawManifest = removeKeyOnFail(
      result.fetchingFiles.then(() => readPkgFromDir(path.join(result.inStoreLocation, 'package'))),
    )
  }

  return result

  function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    return p.catch((err) => {
      ctx.fetchingLocker.delete(opts.pkgId)
      throw err
    })
  }

  async function doFetchToStore (
    fetchingRawManifest: PromiseContainer<PackageJson>,
    fetchingFiles: PromiseContainer<PackageFilesResponse>,
    finishing: PromiseContainer<void>,
  ) {
    try {
      const linkToUnpacked = path.join(target, 'package')

      // We can safely assume that if there is no data about the package in `store.json` then
      // it is not in the store yet.
      // In case there is record about the package in `store.json`, we check it in the file system just in case
      const targetExists = ctx.storeIndex[targetRelative] && await exists(path.join(linkToUnpacked, 'package.json'))

      if (!opts.force && targetExists) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = opts.verifyStoreIntegrity
          ? await checkPackage(linkToUnpacked)
          : await loadJsonFile(path.join(path.dirname(linkToUnpacked), 'integrity.json'))
        if (satisfiedIntegrity) {
          fetchingFiles.resolve({
            filenames: Object.keys(satisfiedIntegrity).filter((f) => !satisfiedIntegrity[f].isDir), // Filtering can be removed for store v3
            fromStore: true,
          })
          if (opts.fetchRawManifest) {
            readPkgFromDir(linkToUnpacked)
              .then(fetchingRawManifest.resolve)
              .catch(fetchingRawManifest.reject)
          }
          finishing.resolve(undefined)
          return
        }
        storeLogger.warn(`Refetching ${target} to store. It was either modified or had no integrity checksums`)
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
          const priority = (++ctx.requestsQueue['counter'] % ctx.requestsQueue['concurrency'] === 0 ? -1 : 1) * 1000 // tslint:disable-line

          const fetchedPackage = await ctx.requestsQueue.add(() => ctx.fetch(opts.resolution, target, {
            cachedTarballLocation: path.join(ctx.storePath, opts.pkgId, 'packed.tgz'),
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

      // Ideally, fetchingFiles wouldn't care about when integrity is calculated.
      // However, we can only rename the temp folder once we know the package name.
      // And we cannot rename the temp folder till we're calculating integrities.
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

      let pkgName: string | undefined = opts.pkgName
      if (!pkgName || opts.fetchRawManifest) {
        const pkg = await readPkgFromDir(tempLocation)
        fetchingRawManifest.resolve(pkg)
        if (!pkgName) {
          pkgName = pkg.name
        }
      }

      const unpacked = path.join(target, 'node_modules', pkgName)
      await mkdirp(path.dirname(unpacked))

      // rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await renameOverwrite(tempLocation, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      ctx.storeIndex[targetRelative] = ctx.storeIndex[targetRelative] || []
      fetchingFiles.resolve({
        filenames: Object.keys(filesIndex).filter((f) => !filesIndex[f].isDir), // Filtering can be removed for store v3
        fromStore: false,
      })
    } catch (err) {
      fetchingFiles.reject(err)
      if (opts.fetchRawManifest) {
        fetchingRawManifest.reject(err)
      }
    }
  }
}

// tslint:disable-next-line
function noop () {}

interface PromiseContainer <T> {
  promise: Promise<T>,
  resolve: (v: T) => void,
  reject: (err: Error) => void,
}

function differed<T> (): PromiseContainer<T> {
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
    storeLogger.warn(`Fetching ${opts.pkgId} failed!`)
    throw err
  }
}

// TODO: It might make sense to have this function as part of storeController.
//  Ask @etamponi if it is fine for when pnpm is used as a server
// TODO: cover with tests
export async function getCacheByEngine (storePath: string, id: string): Promise<Map<string, string>> {
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
