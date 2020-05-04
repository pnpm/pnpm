import createCafs, {
  checkFilesIntegrity as _checkFilesIntegrity,
  FileType,
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  getFilePathInCafs as _getFilePathInCafs,
} from '@pnpm/cafs'
import { fetchingProgressLogger } from '@pnpm/core-loggers'
import {
  Cafs,
  DeferredManifestPromise,
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetcher-base'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import readPackage from '@pnpm/read-package-json'
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
import loadJsonFile = require('load-json-file')
import * as fs from 'mz/fs'
import pDefer = require('p-defer')
import PQueue from 'p-queue'
import path = require('path')
import pShare = require('promise-share')
import R = require('ramda')
import ssri = require('ssri')
import writeJsonFile = require('write-json-file')
import safeDeferredPromise from './safeDeferredPromise'

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
  'version',
])

export default function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  opts: {
    ignoreFile?: (filename: string) => boolean,
    networkConcurrency?: number,
    storeDir: string,
    storeIndex: StoreIndex,
    verifyStoreIntegrity: boolean,
  },
): RequestPackageFunction & {
  cafs: Cafs,
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

  const cafsDir = path.join(opts.storeDir, 'files')
  const cafs = createCafs(cafsDir, opts.ignoreFile)
  const getFilePathInCafs = _getFilePathInCafs.bind(null, cafsDir)
  const fetch = fetcher.bind(null, fetchers, cafs)
  const fetchPackageToStore = fetchToStore.bind(null, {
    checkFilesIntegrity: _checkFilesIntegrity.bind(null, cafsDir),
    fetch,
    fetchingLocker: new Map(),
    getFilePathByModeInCafs: _getFilePathByModeInCafs.bind(null, cafsDir),
    getFilePathInCafs,
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

  return Object.assign(requestPackage, { cafs, fetchPackageToStore, requestPackage })
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
        alwaysTryWorkspacePackages: options.alwaysTryWorkspacePackages,
        defaultTag: options.defaultTag,
        lockfileDir: options.lockfileDir,
        preferredVersions: options.preferredVersions,
        projectDir: options.projectDir,
        registry: options.registry,
        workspacePackages: options.workspacePackages,
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
      lockfileDir: options.lockfileDir,
      pkgId: id,
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
    checkFilesIntegrity: (
      integrity: Record<string, { size: number, mode: number, integrity: string }>,
      manifest?: DeferredManifestPromise,
    ) => Promise<boolean>,
    fetch: (
      packageId: string,
      resolution: Resolution,
      opts: FetchOptions,
    ) => Promise<FetchResult>,
    fetchingLocker: Map<string, {
      finishing: Promise<void>,
      files: Promise<PackageFilesResponse>,
      bundledManifest?: Promise<BundledManifest>,
      inStoreLocation: string,
    }>,
    getFilePathInCafs: (integrity: string, fileType: FileType) => string,
    getFilePathByModeInCafs: (integrity: string, mode: number) => string,
    requestsQueue: {add: <T>(fn: () => Promise<T>, opts: {priority: number}) => Promise<T>},
    storeIndex: StoreIndex,
    storeDir: string,
    verifyStoreIntegrity: boolean,
  },
  opts: {
    fetchRawManifest?: boolean,
    force: boolean,
    pkgId: string,
    lockfileDir: string,
    resolution: Resolution,
  },
): {
  files: () => Promise<PackageFilesResponse>,
  bundledManifest?: () => Promise<BundledManifest>,
  finishing: () => Promise<void>,
  inStoreLocation: string,
} {
  const targetRelative = pkgIdToFilename(opts.pkgId, opts.lockfileDir)
  const target = path.join(ctx.storeDir, targetRelative)

  if (!ctx.fetchingLocker.has(opts.pkgId)) {
    const bundledManifest = pDefer<BundledManifest>()
    const files = pDefer<PackageFilesResponse>()
    const finishing = pDefer<void>()

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
    files.promise.then(({ filesIndex, fromStore }) => { // tslint:disable-line
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
          filesIndex,
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
      result.files.then(({ filesIndex }) => {
        const { integrity, mode } = filesIndex['package.json']
        const manifestPath = ctx.getFilePathByModeInCafs(integrity, mode)
        return readBundledManifest(manifestPath)
      }),
    )
  }

  return {
    bundledManifest: result.bundledManifest ? pShare(result.bundledManifest) : undefined,
    files: pShare(result.files),
    finishing: pShare(result.finishing),
    inStoreLocation: result.inStoreLocation,
  }

  async function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    try {
      return await p
    } catch (err) {
      ctx.fetchingLocker.delete(opts.pkgId)
      throw err
    }
  }

  async function doFetchToStore (
    bundledManifest: pDefer.DeferredPromise<BundledManifest>,
    files: pDefer.DeferredPromise<PackageFilesResponse>,
    finishing: pDefer.DeferredPromise<void>,
  ) {
    try {
      const isLocalTarballDep = opts.pkgId.startsWith('file:')
      const pkgIndexFilePath = opts.resolution['integrity']
        ? ctx.getFilePathInCafs(opts.resolution['integrity'], 'index')
        : path.join(target, 'integrity.json')

      if (
        !opts.force &&
        (
          isLocalTarballDep === false ||
          await tarballIsUpToDate(opts.resolution as any, target, opts.lockfileDir) // tslint:disable-line
        )
      ) {
        let integrity
        try {
          integrity = await loadJsonFile<Record<string, { size: number, mode: number, integrity: string }>>(pkgIndexFilePath)
        } catch (err) {
          // ignoring. It is fine if the integrity file is not present. Just refetch the package
        }
        // if target exists and it wasn't modified, then no need to refetch it

        if (integrity) {
          const manifest = opts.fetchRawManifest
            ? safeDeferredPromise<DependencyManifest>()
            : undefined
          const verified = await ctx.checkFilesIntegrity(integrity, manifest)
          if (verified) {
            files.resolve({
              filesIndex: integrity,
              fromStore: true,
            })
            if (manifest) {
              manifest()
                .then((manifest) => bundledManifest.resolve(pickBundledManifest(manifest)))
                .catch(bundledManifest.reject)
            }
            finishing.resolve(undefined)
            return
          }
          packageRequestLogger.warn({
            message: `Refetching ${target} to store. It was either modified or had no integrity checksums`,
            prefix: opts.lockfileDir,
          })
        }
      }

      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      // Tarballs are requested first because they are bigger than metadata files.
      // However, when one line is left available, allow it to be picked up by a metadata request.
      // This is done in order to avoid situations when tarballs are downloaded in chunks
      // As much tarballs should be downloaded simultaneously as possible.
      const priority = (++ctx.requestsQueue['counter'] % ctx.requestsQueue['concurrency'] === 0 ? -1 : 1) * 1000 // tslint:disable-line

      const fetchManifest = opts.fetchRawManifest
        ? safeDeferredPromise<DependencyManifest>()
        : undefined
      if (fetchManifest) {
        fetchManifest()
          .then((manifest) => bundledManifest.resolve(pickBundledManifest(manifest)))
          .catch(bundledManifest.reject)
      }
      const fetchedPackage = await ctx.requestsQueue.add(() => ctx.fetch(
        opts.pkgId,
        opts.resolution,
        {
          cachedTarballLocation: path.join(ctx.storeDir, opts.pkgId, 'packed.tgz'),
          lockfileDir: opts.lockfileDir,
          manifest: fetchManifest,
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
        },
      ), { priority })

      const filesIndex = fetchedPackage.filesIndex

      // Ideally, files wouldn't care about when integrity is calculated.
      // However, we can only rename the temp folder once we know the package name.
      // And we cannot rename the temp folder till we're calculating integrities.
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
      await writeJsonFile(pkgIndexFilePath, integrity)
      finishing.resolve(undefined)

      if (isLocalTarballDep && opts.resolution['integrity']) { // tslint:disable-line:no-string-literal
        await fs.mkdir(target, { recursive: true })
        await fs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), opts.resolution['integrity'], 'utf8') // tslint:disable-line:no-string-literal
      }

      ctx.storeIndex[targetRelative] = ctx.storeIndex[targetRelative] || []
      files.resolve({
        filesIndex: integrity,
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

async function readBundledManifest (pkgJsonPath: string): Promise<BundledManifest> {
  return pickBundledManifest(await readPackage(pkgJsonPath) as DependencyManifest)
}

async function tarballIsUpToDate (
  resolution: {
    integrity?: string,
    registry?: string,
    tarball: string,
  },
  pkgInStoreLocation: string,
  lockfileDir: string,
) {
  let currentIntegrity!: string
  try {
    currentIntegrity = (await fs.readFile(path.join(pkgInStoreLocation, TARBALL_INTEGRITY_FILENAME), 'utf8'))
  } catch (err) {
    return false
  }
  if (resolution.integrity && currentIntegrity !== resolution.integrity) return false

  const tarball = path.join(lockfileDir, resolution.tarball.slice(5))
  const tarballStream = fs.createReadStream(tarball)
  try {
    return Boolean(await ssri.checkStream(tarballStream, currentIntegrity))
  } catch (err) {
    return false
  }
}

async function fetcher (
  fetcherByHostingType: {[hostingType: string]: FetchFunction},
  cafs: Cafs,
  packageId: string,
  resolution: Resolution,
  opts: FetchOptions,
): Promise<FetchResult> {
  const fetch = fetcherByHostingType[resolution.type || 'tarball']
  if (!fetch) {
    throw new Error(`Fetching for dependency type "${resolution.type}" is not supported`)
  }
  try {
    return await fetch(cafs, resolution, opts)
  } catch (err) {
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.lockfileDir,
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
