import checkPackage from '@pnpm/check-package'
import { fetchingProgressLogger } from '@pnpm/core-loggers'
import {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { fromDir as readPkgFromDir } from '@pnpm/read-package-json'
import {
  DirectoryResolution,
  Resolution,
  ResolveFunction,
  ResolveResult,
} from '@pnpm/resolver-base'
import {
  BundledManifest,
  FetchPackageToStoreFunction,
  PackageFilesResponse,
  PackageResponse,
  RequestPackageFunction,
  RequestPackageOptions,
  WantedDependency,
} from '@pnpm/store-controller-types'
import {
  DependencyManifest,
  StoreIndex,
} from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import makeDir = require('make-dir')
import * as fs from 'mz/fs'
import PQueue from 'p-queue'
import path = require('path')
import exists = require('path-exists')
import pShare = require('promise-share')
import R = require('ramda')
import renameOverwrite = require('rename-overwrite')
import ssri = require('ssri')
import symlinkDir = require('symlink-dir')
import writeJsonFile = require('write-json-file')

const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'
const packageRequestLogger = logger('package-requester')

const pickBundledManifest = R.pick([
  'bin',
  'bundledDependencies',
  'bundleDependencies',
  'dependencies',
  'directories',
  'engines',
  'name',
  'optionalDependencies',
  'os',
  'peerDependencies',
  'peerDependenciesMeta',
  'scripts',
  'version'
])

export default function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  opts: {
    networkConcurrency?: number,
    storeDir: string,
    storeIndex: StoreIndex,
    verifyStoreIntegrity: boolean,
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
    storeDir: opts.storeDir,
    storeIndex: opts.storeIndex,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
  })
  const requestPackage = resolveAndFetch.bind(null, {
    fetchPackageToStore,
    requestsQueue,
    resolve,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
  })

  return Object.assign(requestPackage, { fetchPackageToStore, requestPackage })
}

async function resolveAndFetch (
  ctx: {
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    resolve: ResolveFunction,
    fetchPackageToStore: FetchPackageToStoreFunction,
    storeDir: string,
    verifyStoreIntegrity: boolean,
  },
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
): Promise<PackageResponse> {
  try {
    let latest: string | undefined
    let manifest: DependencyManifest | undefined
    let normalizedPref: string | undefined
    let resolution = options.currentResolution as Resolution
    let pkgId = options.currentPackageId
    const skipResolution = resolution && !options.update
    let forceFetch = false
    let updated = false
    let resolvedVia: string | undefined

    // When fetching is skipped, resolution cannot be skipped.
    // We need the package's manifest when doing `lockfile-only` installs.
    // When we don't fetch, the only way to get the package's manifest is via resolving it.
    //
    // The resolution step is never skipped for local dependencies.
    if (!skipResolution || options.skipFetch || pkgId?.startsWith('file:')) {
      const resolveResult = await ctx.requestsQueue.add<ResolveResult>(() => ctx.resolve(wantedDependency, {
        defaultTag: options.defaultTag,
        localPackages: options.localPackages,
        lockfileDir: options.lockfileDir,
        preferredVersions: options.preferredVersions,
        prefix: options.prefix,
        registry: options.registry,
      }), { priority: options.downloadPriority })

      manifest = resolveResult.manifest
      latest = resolveResult.latest
      resolvedVia = resolveResult.resolvedVia

      // If the integrity of a local tarball dependency has changed,
      // the local tarball should be unpacked, so a fetch to the store should be forced
      forceFetch = Boolean(
        options.currentResolution &&
        pkgId?.startsWith('file:') &&
        options.currentResolution['integrity'] !== resolveResult.resolution['integrity'], // tslint:disable-line:no-string-literal
      )

      if (!skipResolution || forceFetch) {
        updated = pkgId !== resolveResult.id || !resolution || forceFetch
        // Keep the lockfile resolution when possible
        // to keep the original shasum.
        if (updated) {
          resolution = resolveResult.resolution
        }
        pkgId = resolveResult.id
        normalizedPref = resolveResult.normalizedPref
      }
    }

    const id = pkgId as string

    if (resolution.type === 'directory') {
      if (!manifest) {
        throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref}`)
      }
      return {
        body: {
          id,
          isLocal: true,
          manifest,
          normalizedPref,
          resolution: resolution as DirectoryResolution,
          resolvedVia,
          updated,
        },
      }
    }

    // We can skip fetching the package only if the manifest
    // is present after resolution
    if (options.skipFetch && manifest) {
      return {
        body: {
          cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storeDir, id) : new Map(),
          id,
          inStoreLocation: path.join(ctx.storeDir, pkgIdToFilename(id, options.lockfileDir)),
          isLocal: false as const,
          latest,
          manifest,
          normalizedPref,
          resolution,
          resolvedVia,
          updated,
        },
      }
    }

    const fetchResult = ctx.fetchPackageToStore({
      fetchRawManifest: updated || !manifest,
      force: forceFetch,
      pkgId: id,
      pkgName: manifest?.name,
      prefix: options.lockfileDir,
      resolution: resolution,
    })

    return {
      body: {
        cacheByEngine: options.sideEffectsCache ? await getCacheByEngine(ctx.storeDir, id) : new Map(),
        id,
        inStoreLocation: fetchResult.inStoreLocation,
        isLocal: false as const,
        latest,
        manifest,
        normalizedPref,
        resolution,
        resolvedVia,
        updated,
      },
      bundledManifest: fetchResult.bundledManifest,
      files: fetchResult.files,
      finishing: fetchResult.finishing,
    } as PackageResponse
  } catch (err) {
    throw err
  }
}

function fetchToStore (
  ctx: {
    fetch: (
      packageId: string,
      resolution: Resolution,
      target: string,
      opts: FetchOptions
    ) => Promise<FetchResult>,
    fetchingLocker: Map<string, {
      finishing: Promise<void>,
      files: Promise<PackageFilesResponse>,
      bundledManifest?: Promise<BundledManifest>,
      inStoreLocation: string,
    }>,
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    storeIndex: StoreIndex,
    storeDir: string,
    verifyStoreIntegrity: boolean,
  },
  opts: {
    fetchRawManifest?: boolean,
    force: boolean,
    pkgName?: string,
    pkgId: string,
    prefix: string,
    resolution: Resolution,
  },
): {
  files: () => Promise<PackageFilesResponse>,
  bundledManifest?: () => Promise<BundledManifest>,
  finishing: () => Promise<void>,
  inStoreLocation: string,
} {
  const targetRelative = pkgIdToFilename(opts.pkgId, opts.prefix)
  const target = path.join(ctx.storeDir, targetRelative)

  if (!ctx.fetchingLocker.has(opts.pkgId)) {
    const bundledManifest = differed<BundledManifest>()
    const files = differed<PackageFilesResponse>()
    const finishing = differed<void>()

    doFetchToStore(bundledManifest, files, finishing) // tslint:disable-line

    if (opts.fetchRawManifest) {
      ctx.fetchingLocker.set(opts.pkgId, {
        bundledManifest: removeKeyOnFail(bundledManifest.promise),
        files: removeKeyOnFail(files.promise),
        finishing: removeKeyOnFail(finishing.promise),
        inStoreLocation: target,
      })
    } else {
      ctx.fetchingLocker.set(opts.pkgId, {
        files: removeKeyOnFail(files.promise),
        finishing: removeKeyOnFail(finishing.promise),
        inStoreLocation: target,
      })
    }

    // When files resolves, the cached result has to set fromStore to true, without
    // affecting previous invocations: so we need to replace the cache.
    //
    // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
    // Otherwise, if a package was not in store when the server started, it will be always
    // reported as "downloaded" instead of "reused".
    files.promise.then(({ filenames, fromStore }) => { // tslint:disable-line
      // If it's already in the store, we don't need to update the cache
      if (fromStore) {
        return
      }

      const tmp = ctx.fetchingLocker.get(opts.pkgId) as {
        files: Promise<PackageFilesResponse>,
        bundledManifest?: Promise<BundledManifest>,
        finishing: Promise<void>,
        inStoreLocation: string,
      }

      // If fetching failed then it was removed from the cache.
      // It is OK. In that case there is no need to update it.
      if (!tmp) return

      ctx.fetchingLocker.set(opts.pkgId, {
        bundledManifest: tmp.bundledManifest,
        files: Promise.resolve({
          filenames,
          fromStore: true,
        }),
        finishing: tmp.finishing,
        inStoreLocation: tmp.inStoreLocation,
      })
    })
    .catch(() => {
      ctx.fetchingLocker.delete(opts.pkgId)
    })
  }

  const result = ctx.fetchingLocker.get(opts.pkgId) as {
    files: Promise<PackageFilesResponse>,
    bundledManifest?: Promise<BundledManifest>,
    finishing: Promise<void>,
    inStoreLocation: string,
  }

  if (opts.fetchRawManifest && !result.bundledManifest) {
    result.bundledManifest = removeKeyOnFail(
      result.files.then(() => readBundledManifest(path.join(result.inStoreLocation, 'package'))),
    )
  }

  return {
    bundledManifest: result.bundledManifest ? pShare(result.bundledManifest) : undefined,
    files: pShare(result.files),
    finishing: pShare(result.finishing),
    inStoreLocation: result.inStoreLocation,
  }

  function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    return p.catch((err) => {
      ctx.fetchingLocker.delete(opts.pkgId)
      throw err
    })
  }

  async function doFetchToStore (
    bundledManifest: PromiseContainer<BundledManifest>,
    files: PromiseContainer<PackageFilesResponse>,
    finishing: PromiseContainer<void>,
  ) {
    try {
      const isLocalTarballDep = opts.pkgId.startsWith('file:')
      const linkToUnpacked = path.join(target, 'package')

      // We can safely assume that if there is no data about the package in `store.json` then
      // it is not in the store yet.
      // In case there is record about the package in `store.json`, we check it in the file system just in case
      const targetExists = ctx.storeIndex[targetRelative] && await exists(path.join(linkToUnpacked, 'package.json'))

      if (
        !opts.force && targetExists &&
        (
          isLocalTarballDep === false ||
          await tarballIsUpToDate(opts.resolution as any, target, opts.prefix) // tslint:disable-line
        )
      ) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = ctx.verifyStoreIntegrity
          ? await checkPackage(linkToUnpacked)
          : await loadJsonFile<object>(path.join(path.dirname(linkToUnpacked), 'integrity.json'))
        if (satisfiedIntegrity) {
          files.resolve({
            filenames: Object.keys(satisfiedIntegrity).filter((f) => !satisfiedIntegrity[f].isDir), // Filtering can be removed for store v3
            fromStore: true,
          })
          if (opts.fetchRawManifest) {
            readBundledManifest(linkToUnpacked)
              .then(bundledManifest.resolve)
              .catch(bundledManifest.reject)
          }
          finishing.resolve(undefined)
          return
        }
        packageRequestLogger.warn({
          message: `Refetching ${target} to store. It was either modified or had no integrity checksums`,
          prefix: opts.prefix,
        })
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

          const fetchedPackage = await ctx.requestsQueue.add(() => ctx.fetch(
            opts.pkgId,
            opts.resolution,
            target,
            {
              cachedTarballLocation: path.join(ctx.storeDir, opts.pkgId, 'packed.tgz'),
              onProgress: (downloaded) => {
                fetchingProgressLogger.debug({
                  downloaded,
                  packageId: opts.pkgId,
                  status: 'in_progress',
                })
              },
              onStart: (size, attempt) => {
                fetchingProgressLogger.debug({
                  attempt,
                  packageId: opts.pkgId,
                  size,
                  status: 'started',
                })
              },
              prefix: opts.prefix,
            },
          ), { priority })

          filesIndex = fetchedPackage.filesIndex
          tempLocation = fetchedPackage.tempLocation
        })(),
        // removing only the folder with the unpacked files
        // not touching tarball and integrity.json
        targetExists && await rimraf(path.join(target, 'node_modules')),
      ])

      // Ideally, files wouldn't care about when integrity is calculated.
      // However, we can only rename the temp folder once we know the package name.
      // And we cannot rename the temp folder till we're calculating integrities.
      if (ctx.verifyStoreIntegrity) {
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
        await writeJsonFile(path.join(target, 'integrity.json'), integrity, { indent: undefined })
      } else {
        // TODO: save only filename: {size}
        await writeJsonFile(path.join(target, 'integrity.json'), filesIndex, { indent: undefined })
      }
      finishing.resolve(undefined)

      let pkgName: string | undefined = opts.pkgName
      if (!pkgName || opts.fetchRawManifest) {
        const manifest = await readPkgFromDir(tempLocation) as DependencyManifest
        bundledManifest.resolve(pickBundledManifest(manifest))
        if (!pkgName) {
          pkgName = manifest.name
        }
      }

      const unpacked = path.join(target, 'node_modules', pkgName)
      await makeDir(path.dirname(unpacked))

      // rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await renameOverwrite(tempLocation, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      if (isLocalTarballDep && opts.resolution['integrity']) { // tslint:disable-line:no-string-literal
        await fs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), opts.resolution['integrity'], 'utf8') // tslint:disable-line:no-string-literal
      }

      ctx.storeIndex[targetRelative] = ctx.storeIndex[targetRelative] || []
      files.resolve({
        filenames: Object.keys(filesIndex).filter((f) => !filesIndex[f].isDir), // Filtering can be removed for store v3
        fromStore: false,
      })
    } catch (err) {
      files.reject(err)
      if (opts.fetchRawManifest) {
        bundledManifest.reject(err)
      }
    }
  }
}

async function readBundledManifest (dir: string): Promise<BundledManifest> {
  return pickBundledManifest(await readPkgFromDir(dir) as DependencyManifest)
}

async function tarballIsUpToDate (
  resolution: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  pkgInStoreLocation: string,
  prefix: string,
) {
  let currentIntegrity!: string
  try {
    currentIntegrity = (await fs.readFile(path.join(pkgInStoreLocation, TARBALL_INTEGRITY_FILENAME), 'utf8'))
  } catch (err) {
    return false
  }
  if (resolution.integrity && currentIntegrity !== resolution.integrity) return false

  const tarball = path.join(prefix, resolution.tarball.slice(5))
  const tarballStream = fs.createReadStream(tarball)
  try {
    return Boolean(await ssri.checkStream(tarballStream, currentIntegrity))
  } catch (err) {
    return false
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
  packageId: string,
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
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.prefix,
    })
    throw err
  }
}

// TODO: cover with tests
export async function getCacheByEngine (storeDir: string, id: string): Promise<Map<string, string>> {
  const map = new Map<string, string>()

  const cacheRoot = path.join(storeDir, id, 'side_effects')
  if (!await fs.exists(cacheRoot)) {
    return map
  }

  const dirContents = (await fs.readdir(cacheRoot)).map((content: string) => path.join(cacheRoot, content))
  await Promise.all(dirContents.map(async (dir: string) => {
    if (!(await fs.lstat(dir)).isDirectory()) {
      return
    }
    const engineName = path.basename(dir)
    map[engineName] = path.join(dir, 'package')
  }))

  return map
}
