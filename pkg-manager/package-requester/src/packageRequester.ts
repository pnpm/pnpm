import { createReadStream, promises as fs } from 'fs'
import os from 'os'
import path from 'path'
import {
  type FileType,
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  getFilePathInCafs as _getFilePathInCafs,
  type PackageFilesIndex,
} from '@pnpm/store.cafs'
import { fetchingProgressLogger, progressLogger } from '@pnpm/core-loggers'
import { pickFetcher } from '@pnpm/pick-fetcher'
import { PnpmError } from '@pnpm/error'
import {
  type DirectoryFetcherResult,
  type Fetchers,
  type FetchOptions,
  type FetchResult,
} from '@pnpm/fetcher-base'
import { type Cafs } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { globalWarn, logger } from '@pnpm/logger'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { readPackageJson } from '@pnpm/read-package-json'
import {
  type DirectoryResolution,
  type Resolution,
  type ResolveFunction,
  type ResolveResult,
  type TarballResolution,
} from '@pnpm/resolver-base'
import {
  type BundledManifest,
  type PkgRequestFetchResult,
  type FetchPackageToStoreFunction,
  type FetchPackageToStoreOptions,
  type GetFilesIndexFilePath,
  type PackageResponse,
  type PkgNameVersion,
  type RequestPackageFunction,
  type RequestPackageOptions,
  type WantedDependency,
} from '@pnpm/store-controller-types'
import { type DependencyManifest } from '@pnpm/types'
import { depPathToFilename } from '@pnpm/dependency-path'
import { readPkgFromCafs as _readPkgFromCafs } from '@pnpm/worker'
import PQueue from 'p-queue'
import pDefer from 'p-defer'
import pShare from 'promise-share'
import pick from 'ramda/src/pick'
import semver from 'semver'
import ssri from 'ssri'
import { equalOrSemverEqual } from './equalOrSemverEqual'

const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'
const packageRequestLogger = logger('package-requester')

const pickBundledManifest = pick([
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

function normalizeBundledManifest (manifest: DependencyManifest): BundledManifest {
  return {
    ...pickBundledManifest(manifest),
    version: semver.clean(manifest.version ?? '0.0.0', { loose: true }) ?? manifest.version,
  }
}

export function createPackageRequester (
  opts: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    resolve: ResolveFunction
    fetchers: Fetchers
    cafs: Cafs
    ignoreFile?: (filename: string) => boolean
    networkConcurrency?: number
    storeDir: string
    verifyStoreIntegrity: boolean
    virtualStoreDirMaxLength: number
    strictStorePkgContentCheck?: boolean
  }
): RequestPackageFunction & {
    fetchPackageToStore: FetchPackageToStoreFunction
    getFilesIndexFilePath: GetFilesIndexFilePath
    requestPackage: RequestPackageFunction
  } {
  opts = opts || {}

  // A lower bound of 16 is enforced to prevent performance degradation,
  // especially in CI environments. Tests with a threshold lower than 16
  // have shown consistent underperformance.
  const networkConcurrency = opts.networkConcurrency ?? Math.max(os.availableParallelism?.() ?? os.cpus().length, 16)
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  const cafsDir = path.join(opts.storeDir, 'files')
  const getFilePathInCafs = _getFilePathInCafs.bind(null, cafsDir)
  const fetch = fetcher.bind(null, opts.fetchers, opts.cafs)
  const fetchPackageToStore = fetchToStore.bind(null, {
    readPkgFromCafs: _readPkgFromCafs.bind(null, cafsDir, opts.verifyStoreIntegrity),
    fetch,
    fetchingLocker: new Map(),
    getFilePathByModeInCafs: _getFilePathByModeInCafs.bind(null, cafsDir),
    getFilePathInCafs,
    requestsQueue: Object.assign(requestsQueue, {
      counter: 0,
      concurrency: networkConcurrency,
    }),
    storeDir: opts.storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
  })
  const requestPackage = resolveAndFetch.bind(null, {
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.pnpmVersion,
    force: opts.force,
    fetchPackageToStore,
    requestsQueue,
    resolve: opts.resolve,
    storeDir: opts.storeDir,
  })

  return Object.assign(requestPackage, {
    fetchPackageToStore,
    getFilesIndexFilePath: getFilesIndexFilePath.bind(null, {
      getFilePathInCafs,
      storeDir: opts.storeDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }),
    requestPackage,
  })
}

async function resolveAndFetch (
  ctx: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    pnpmVersion?: string
    requestsQueue: { add: <T>(fn: () => Promise<T>, opts: { priority: number }) => Promise<T> }
    resolve: ResolveFunction
    fetchPackageToStore: FetchPackageToStoreFunction
    storeDir: string
  },
  wantedDependency: WantedDependency & { optional?: boolean },
  options: RequestPackageOptions
): Promise<PackageResponse> {
  let latest: string | undefined
  let manifest: DependencyManifest | undefined
  let normalizedPref: string | undefined
  let resolution = options.currentPkg?.resolution as Resolution
  let pkgId = options.currentPkg?.id
  const skipResolution = resolution && !options.update
  let forceFetch = false
  let updated = false
  let resolvedVia: string | undefined
  let publishedAt: string | undefined

  // When fetching is skipped, resolution cannot be skipped.
  // We need the package's manifest when doing `lockfile-only` installs.
  // When we don't fetch, the only way to get the package's manifest is via resolving it.
  //
  // The resolution step is never skipped for local dependencies.
  if (!skipResolution || options.skipFetch === true || Boolean(pkgId?.startsWith('file:')) || wantedDependency.optional === true) {
    const resolveResult = await ctx.requestsQueue.add<ResolveResult>(async () => ctx.resolve(wantedDependency, {
      alwaysTryWorkspacePackages: options.alwaysTryWorkspacePackages,
      defaultTag: options.defaultTag,
      publishedBy: options.publishedBy,
      pickLowestVersion: options.pickLowestVersion,
      lockfileDir: options.lockfileDir,
      preferredVersions: options.preferredVersions,
      preferWorkspacePackages: options.preferWorkspacePackages,
      projectDir: options.projectDir,
      registry: options.registry,
      workspacePackages: options.workspacePackages,
      updateToLatest: options.updateToLatest,
    }), { priority: options.downloadPriority })

    manifest = resolveResult.manifest
    latest = resolveResult.latest
    resolvedVia = resolveResult.resolvedVia
    publishedAt = resolveResult.publishedAt

    // If the integrity of a local tarball dependency has changed,
    // the local tarball should be unpacked, so a fetch to the store should be forced
    forceFetch = Boolean(
      ((options.currentPkg?.resolution) != null) &&
      pkgId?.startsWith('file:') &&
      (options.currentPkg?.resolution as TarballResolution).integrity !== (resolveResult.resolution as TarballResolution).integrity
    )

    updated = pkgId !== resolveResult.id || !resolution || forceFetch
    resolution = resolveResult.resolution
    pkgId = resolveResult.id
    normalizedPref = resolveResult.normalizedPref
  }

  const id = pkgId!

  if (resolution.type === 'directory' && !id.startsWith('file:')) {
    if (manifest == null) {
      throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.pref ?? ''}`)
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

  const isInstallable = (
    ctx.force === true ||
      (
        manifest == null
          ? undefined
          : packageIsInstallable(id, manifest, {
            engineStrict: ctx.engineStrict,
            lockfileDir: options.lockfileDir,
            nodeVersion: ctx.nodeVersion,
            optional: wantedDependency.optional === true,
            supportedArchitectures: options.supportedArchitectures,
          })
      )
  )
  // We can skip fetching the package only if the manifest
  // is present after resolution
  if ((options.skipFetch === true || isInstallable === false) && (manifest != null)) {
    return {
      body: {
        id,
        isLocal: false as const,
        isInstallable: isInstallable ?? undefined,
        latest,
        manifest,
        normalizedPref,
        resolution,
        resolvedVia,
        updated,
        publishedAt,
      },
    }
  }

  const pkg: PkgNameVersion = manifest != null ? pick(['name', 'version'], manifest) : {}
  const fetchResult = ctx.fetchPackageToStore({
    fetchRawManifest: true,
    force: forceFetch,
    ignoreScripts: options.ignoreScripts,
    lockfileDir: options.lockfileDir,
    pkg: {
      ...pkg,
      id,
      resolution,
    },
    expectedPkg: options.expectedPkg?.name != null
      ? (updated ? { name: options.expectedPkg.name, version: pkg.version } : options.expectedPkg)
      : pkg,
    onFetchError: options.onFetchError,
  })

  if (!manifest) {
    manifest = (await fetchResult.fetching()).bundledManifest
  }
  return {
    body: {
      id,
      isLocal: false as const,
      isInstallable: isInstallable ?? undefined,
      latest,
      manifest,
      normalizedPref,
      resolution,
      resolvedVia,
      updated,
      publishedAt,
    },
    fetching: fetchResult.fetching,
    filesIndexFile: fetchResult.filesIndexFile,
  }
}

interface FetchLock {
  fetching: Promise<PkgRequestFetchResult>
  filesIndexFile: string
  fetchRawManifest?: boolean
}

function getFilesIndexFilePath (
  ctx: {
    getFilePathInCafs: (integrity: string, fileType: FileType) => string
    storeDir: string
    virtualStoreDirMaxLength: number
  },
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts'>
) {
  const targetRelative = depPathToFilename(opts.pkg.id, ctx.virtualStoreDirMaxLength)
  const target = path.join(ctx.storeDir, targetRelative)
  const filesIndexFile = (opts.pkg.resolution as TarballResolution).integrity
    ? ctx.getFilePathInCafs((opts.pkg.resolution as TarballResolution).integrity!, 'index')
    : path.join(target, opts.ignoreScripts ? 'integrity-not-built.json' : 'integrity.json')
  return { filesIndexFile, target }
}

function fetchToStore (
  ctx: {
    readPkgFromCafs: (
      filesIndexFile: string,
      readManifest?: boolean
    ) => Promise<{ verified: boolean, pkgFilesIndex: PackageFilesIndex, manifest?: DependencyManifest, requiresBuild: boolean }>
    fetch: (
      packageId: string,
      resolution: Resolution,
      opts: FetchOptions
    ) => Promise<FetchResult>
    fetchingLocker: Map<string, FetchLock>
    getFilePathInCafs: (integrity: string, fileType: FileType) => string
    getFilePathByModeInCafs: (integrity: string, mode: number) => string
    requestsQueue: {
      add: <T>(fn: () => Promise<T>, opts: { priority: number }) => Promise<T>
      counter: number
      concurrency: number
    }
    storeDir: string
    virtualStoreDirMaxLength: number
    strictStorePkgContentCheck?: boolean
  },
  opts: FetchPackageToStoreOptions
): {
    filesIndexFile: string
    fetching: () => Promise<PkgRequestFetchResult>
  } {
  if (!opts.pkg.name) {
    opts.fetchRawManifest = true
  }

  if (!ctx.fetchingLocker.has(opts.pkg.id)) {
    const fetching = pDefer<PkgRequestFetchResult>()
    const { filesIndexFile, target } = getFilesIndexFilePath(ctx, opts)

    doFetchToStore(filesIndexFile, fetching, target) // eslint-disable-line

    ctx.fetchingLocker.set(opts.pkg.id, {
      fetching: removeKeyOnFail(fetching.promise),
      filesIndexFile,
      fetchRawManifest: opts.fetchRawManifest,
    })

    // When files resolves, the cached result has to set fromStore to true, without
    // affecting previous invocations: so we need to replace the cache.
    //
    // Changing the value of fromStore is needed for correct reporting of `pnpm server`.
    // Otherwise, if a package was not in store when the server started, it will always be
    // reported as "downloaded" instead of "reused".
    fetching.promise.then((cache) => {
      progressLogger.debug({
        packageId: opts.pkg.id,
        requester: opts.lockfileDir,
        status: cache.files.resolvedFrom === 'remote'
          ? 'fetched'
          : 'found_in_store',
      })

      // If it's already in the store, we don't need to update the cache
      if (cache.files.resolvedFrom !== 'remote') {
        return
      }

      const tmp = ctx.fetchingLocker.get(opts.pkg.id)

      // If fetching failed then it was removed from the cache.
      // It is OK. In that case there is no need to update it.
      if (tmp == null) return

      ctx.fetchingLocker.set(opts.pkg.id, {
        ...tmp,
        fetching: Promise.resolve({
          ...cache,
          files: {
            ...cache.files,
            resolvedFrom: 'store',
          },
        }),
      })
    })
      .catch(() => {
        ctx.fetchingLocker.delete(opts.pkg.id)
      })
  }

  const result = ctx.fetchingLocker.get(opts.pkg.id)!

  if (opts.fetchRawManifest && !result.fetchRawManifest) {
    result.fetching = removeKeyOnFail(
      result.fetching.then(async ({ files }) => {
        if (!files.filesIndex['package.json']) return {
          files,
          bundledManifest: undefined,
        }
        if (files.unprocessed) {
          const { integrity, mode } = files.filesIndex['package.json']
          const manifestPath = ctx.getFilePathByModeInCafs(integrity, mode)
          return {
            files,
            bundledManifest: await readBundledManifest(manifestPath),
          }
        }
        return {
          files,
          bundledManifest: await readBundledManifest(files.filesIndex['package.json']),
        }
      })
    )
    result.fetchRawManifest = true
  }

  return {
    fetching: pShare(result.fetching),
    filesIndexFile: result.filesIndexFile,
  }

  async function removeKeyOnFail<T> (p: Promise<T>): Promise<T> {
    try {
      return await p
    } catch (err: any) { // eslint-disable-line
      ctx.fetchingLocker.delete(opts.pkg.id)
      if (opts.onFetchError) {
        throw opts.onFetchError(err)
      }
      throw err
    }
  }

  async function doFetchToStore (
    filesIndexFile: string,
    fetching: pDefer.DeferredPromise<PkgRequestFetchResult>,
    target: string
  ) {
    try {
      const isLocalTarballDep = opts.pkg.id.startsWith('file:')
      const isLocalPkg = opts.pkg.resolution.type === 'directory'

      if (
        !opts.force &&
        (
          !isLocalTarballDep ||
          await tarballIsUpToDate(opts.pkg.resolution as any, target, opts.lockfileDir) // eslint-disable-line
        ) &&
        !isLocalPkg
      ) {
        const { verified, pkgFilesIndex, manifest, requiresBuild } = await ctx.readPkgFromCafs(filesIndexFile, opts.fetchRawManifest)
        if (verified) {
          if (
            (
              pkgFilesIndex.name != null &&
              opts.expectedPkg?.name != null &&
              pkgFilesIndex.name.toLowerCase() !== opts.expectedPkg.name.toLowerCase()
            ) ||
            (
              pkgFilesIndex.version != null &&
              opts.expectedPkg?.version != null &&
              // We used to not normalize the package versions before writing them to the lockfile and store.
              // So it may happen that the version will be in different formats.
              // For instance, v1.0.0 and 1.0.0
              // Hence, we need to use semver.eq() to compare them.
              !equalOrSemverEqual(pkgFilesIndex.version, opts.expectedPkg.version)
            )
          ) {
            const msg = `Package name mismatch found while reading ${JSON.stringify(opts.pkg.resolution)} from the store.`
            const hint = `This means that either the lockfile is broken or the package metadata (name and version) inside the package's package.json file doesn't match the metadata in the registry. \
Expected package: ${opts.expectedPkg.name}@${opts.expectedPkg.version}. \
Actual package in the store with the given integrity: ${pkgFilesIndex.name}@${pkgFilesIndex.version}.`
            if (ctx.strictStorePkgContentCheck ?? true) {
              throw new PnpmError('UNEXPECTED_PKG_CONTENT_IN_STORE', msg, {
                hint: `${hint}\n\nIf you want to ignore this issue, set the strict-store-pkg-content-check to false.`,
              })
            } else {
              globalWarn(`${msg} ${hint}`)
            }
          }
          fetching.resolve({
            files: {
              unprocessed: true,
              filesIndex: pkgFilesIndex.files,
              resolvedFrom: 'store',
              sideEffects: pkgFilesIndex.sideEffects,
              requiresBuild,
            },
            bundledManifest: manifest == null ? manifest : normalizeBundledManifest(manifest),
          })
          return
        }
        if ((pkgFilesIndex?.files) != null) {
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
      // As many tarballs should be downloaded simultaneously as possible.
      const priority = (++ctx.requestsQueue.counter % ctx.requestsQueue.concurrency === 0 ? -1 : 1) * 1000

      const fetchedPackage = await ctx.requestsQueue.add(async () => ctx.fetch(
        opts.pkg.id,
        opts.pkg.resolution,
        {
          filesIndexFile,
          lockfileDir: opts.lockfileDir,
          readManifest: opts.fetchRawManifest,
          onProgress: (downloaded) => {
            fetchingProgressLogger.debug({
              downloaded,
              packageId: opts.pkg.id,
              status: 'in_progress',
            })
          },
          onStart: (size, attempt) => {
            fetchingProgressLogger.debug({
              attempt,
              packageId: opts.pkg.id,
              size,
              status: 'started',
            })
          },
          pkg: {
            name: opts.pkg.name,
            version: opts.pkg.version,
          },
        }
      ), { priority })

      if (isLocalTarballDep && (opts.pkg.resolution as TarballResolution).integrity) {
        await fs.mkdir(target, { recursive: true })
        await gfs.writeFile(path.join(target, TARBALL_INTEGRITY_FILENAME), (opts.pkg.resolution as TarballResolution).integrity!, 'utf8')
      }

      fetching.resolve({
        files: {
          resolvedFrom: fetchedPackage.local ? 'local-dir' : 'remote',
          filesIndex: fetchedPackage.filesIndex,
          packageImportMethod: (fetchedPackage as DirectoryFetcherResult).packageImportMethod,
          requiresBuild: fetchedPackage.requiresBuild,
        },
        bundledManifest: fetchedPackage.manifest == null ? fetchedPackage.manifest : normalizeBundledManifest(fetchedPackage.manifest),
      })
    } catch (err: any) { // eslint-disable-line
      fetching.reject(err)
    }
  }
}

async function readBundledManifest (pkgJsonPath: string): Promise<BundledManifest> {
  return pickBundledManifest(await readPackageJson(pkgJsonPath) as DependencyManifest)
}

async function tarballIsUpToDate (
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  pkgInStoreLocation: string,
  lockfileDir: string
) {
  let currentIntegrity!: string
  try {
    currentIntegrity = (await gfs.readFile(path.join(pkgInStoreLocation, TARBALL_INTEGRITY_FILENAME), 'utf8'))
  } catch (err: any) { // eslint-disable-line
    return false
  }
  if (resolution.integrity && currentIntegrity !== resolution.integrity) return false

  const tarball = path.join(lockfileDir, resolution.tarball.slice(5))
  const tarballStream = createReadStream(tarball)
  try {
    return Boolean(await ssri.checkStream(tarballStream, currentIntegrity))
  } catch (err: any) { // eslint-disable-line
    return false
  }
}

async function fetcher (
  fetcherByHostingType: Fetchers,
  cafs: Cafs,
  packageId: string,
  resolution: Resolution,
  opts: FetchOptions
): Promise<FetchResult> {
  const fetch = pickFetcher(fetcherByHostingType, resolution)
  try {
    return await fetch(cafs, resolution as any, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
  } catch (err: any) { // eslint-disable-line
    packageRequestLogger.warn({
      message: `Fetching ${packageId} failed!`,
      prefix: opts.lockfileDir,
    })
    throw err
  }
}
