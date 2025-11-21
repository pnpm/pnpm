import { createReadStream, promises as fs } from 'fs'
import path from 'path'
import {
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  getIndexFilePathInCafs as _getIndexFilePathInCafs,
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
  type PlatformAssetResolution,
  type DirectoryResolution,
  type PreferredVersions,
  type Resolution,
  type ResolveFunction,
  type ResolveResult,
  type TarballResolution,
  type AtomicResolution,
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
import { type DependencyManifest, type SupportedArchitectures } from '@pnpm/types'
import { depPathToFilename } from '@pnpm/dependency-path'
import { calcMaxWorkers, readPkgFromCafs as _readPkgFromCafs } from '@pnpm/worker'
import { familySync } from 'detect-libc'
import PQueue from 'p-queue'
import pDefer, { type DeferredPromise } from 'p-defer'
import pShare from 'promise-share'
import { pick } from 'ramda'
import semver from 'semver'
import ssri from 'ssri'
import { equalOrSemverEqual } from './equalOrSemverEqual.js'

let currentLibc: 'glibc' | 'musl' | undefined | null
function getLibcFamilySync () {
  if (currentLibc === undefined) {
    currentLibc = familySync() as unknown as typeof currentLibc
  }
  return currentLibc
}
const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'
const packageRequestLogger = logger('package-requester')

const pickBundledManifest = pick([
  'bin',
  'bundledDependencies',
  'bundleDependencies',
  'cpu',
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

  const networkConcurrency = opts.networkConcurrency ?? Math.min(64, Math.max(calcMaxWorkers() * 3, 16))
  const requestsQueue = new PQueue({
    concurrency: networkConcurrency,
  })

  const getIndexFilePathInCafs = _getIndexFilePathInCafs.bind(null, opts.storeDir)
  const fetch = fetcher.bind(null, opts.fetchers, opts.cafs)
  const fetchPackageToStore = fetchToStore.bind(null, {
    readPkgFromCafs: _readPkgFromCafs.bind(null, opts.storeDir, opts.verifyStoreIntegrity),
    fetch,
    fetchingLocker: new Map(),
    getFilePathByModeInCafs: _getFilePathByModeInCafs.bind(null, opts.storeDir),
    getIndexFilePathInCafs,
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
      getIndexFilePathInCafs,
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
  let normalizedBareSpecifier: string | undefined
  let alias: string | undefined
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
    // When skipResolution is set but a resolution is still performed due to
    // options.skipFetch, it's necessary to make sure the resolution doesn't
    // accidentally return a newer version of the package. When skipFetch is
    // set, the resolved package shouldn't be different. This is done by
    // overriding the preferredVersions object to only contain the current
    // package's version.
    //
    // A naive approach would be to change the bare specifier to be the exact
    // version of the current pkg if the bare specifier is a range, but this
    // would cause the version returned for calcSpecifier to be different.
    const preferredVersions: PreferredVersions = (skipResolution && options.currentPkg?.name != null && options.currentPkg?.version != null)
      ? {
        ...options.preferredVersions,
        [options.currentPkg.name]: { [options.currentPkg.version]: 'version' },
      }
      : options.preferredVersions

    const resolveResult = await ctx.requestsQueue.add<ResolveResult>(async () => ctx.resolve(wantedDependency, {
      alwaysTryWorkspacePackages: options.alwaysTryWorkspacePackages,
      defaultTag: options.defaultTag,
      trustPolicy: options.trustPolicy,
      trustPolicyExclude: options.trustPolicyExclude,
      publishedBy: options.publishedBy,
      publishedByExclude: options.publishedByExclude,
      pickLowestVersion: options.pickLowestVersion,
      lockfileDir: options.lockfileDir,
      preferredVersions,
      preferWorkspacePackages: options.preferWorkspacePackages,
      projectDir: options.projectDir,
      workspacePackages: options.workspacePackages,
      update: options.update,
      injectWorkspacePackages: options.injectWorkspacePackages,
      calcSpecifier: options.calcSpecifier,
      pinnedVersion: options.pinnedVersion,
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
    normalizedBareSpecifier = resolveResult.normalizedBareSpecifier
    alias = resolveResult.alias
  }

  const id = pkgId!

  if ('type' in resolution && resolution.type === 'directory' && !id.startsWith('file:')) {
    if (manifest == null) {
      throw new Error(`Couldn't read package.json of local dependency ${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.bareSpecifier ?? ''}`)
    }
    return {
      body: {
        id,
        isLocal: true,
        manifest,
        resolution: resolution as DirectoryResolution,
        resolvedVia,
        updated,
        normalizedBareSpecifier,
        alias,
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
        normalizedBareSpecifier,
        resolution,
        resolvedVia,
        updated,
        publishedAt,
        alias,
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
      ...(options.expectedPkg?.name != null
        ? (updated ? { name: options.expectedPkg.name, version: pkg.version } : options.expectedPkg)
        : pkg
      ),
      id,
      resolution,
    },
    onFetchError: options.onFetchError,
    supportedArchitectures: options.supportedArchitectures,
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
      normalizedBareSpecifier,
      resolution,
      resolvedVia,
      updated,
      publishedAt,
      alias,
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

interface GetFilesIndexFilePathResult {
  target: string
  filesIndexFile: string
  resolution: AtomicResolution
}

function getFilesIndexFilePath (
  ctx: {
    getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
    storeDir: string
    virtualStoreDirMaxLength: number
  },
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts' | 'supportedArchitectures'>
): GetFilesIndexFilePathResult {
  const targetRelative = depPathToFilename(opts.pkg.id, ctx.virtualStoreDirMaxLength)
  const target = path.join(ctx.storeDir, targetRelative)
  if ((opts.pkg.resolution as TarballResolution).integrity) {
    return {
      target,
      filesIndexFile: ctx.getIndexFilePathInCafs((opts.pkg.resolution as TarballResolution).integrity!, opts.pkg.id),
      resolution: opts.pkg.resolution as AtomicResolution,
    }
  }
  let resolution!: AtomicResolution
  if (opts.pkg.resolution.type === 'variations') {
    resolution = findResolution(opts.pkg.resolution.variants, opts.supportedArchitectures)
    if ((resolution as TarballResolution).integrity) {
      return {
        target,
        filesIndexFile: ctx.getIndexFilePathInCafs((resolution as TarballResolution).integrity!, opts.pkg.id),
        resolution,
      }
    }
  } else {
    resolution = opts.pkg.resolution
  }
  const filesIndexFile = path.join(target, opts.ignoreScripts ? 'integrity-not-built.json' : 'integrity.json')
  return { filesIndexFile, target, resolution }
}

function findResolution (resolutionVariants: PlatformAssetResolution[], supportedArchitectures?: SupportedArchitectures): AtomicResolution {
  const platform = getOneIfNonCurrent(supportedArchitectures?.os) ?? process.platform
  const cpu = getOneIfNonCurrent(supportedArchitectures?.cpu) ?? process.arch
  const libc = getOneIfNonCurrent(supportedArchitectures?.libc) ?? getLibcFamilySync()
  const resolutionVariant = resolutionVariants
    .find((resolutionVariant) => resolutionVariant.targets.some(
      (target) =>
        target.os === platform &&
        target.cpu === cpu &&
        (target.libc == null || target.libc === libc)
    ))
  if (!resolutionVariant) {
    const resolutionTargets = resolutionVariants.map((variant) => variant.targets)
    throw new PnpmError('NO_RESOLUTION_MATCHED', `Cannot find a resolution variant for the current platform in these resolutions: ${JSON.stringify(resolutionTargets)}`)
  }
  return resolutionVariant.resolution
}

function getOneIfNonCurrent (requirements: string[] | undefined): string | undefined {
  if (requirements?.length && requirements[0] !== 'current') {
    return requirements[0]
  }
  return undefined
}

function fetchToStore (
  ctx: {
    readPkgFromCafs: (
      filesIndexFile: string,
      readManifest?: boolean
    ) => Promise<{ verified: boolean, pkgFilesIndex: PackageFilesIndex, manifest?: DependencyManifest, requiresBuild: boolean }>
    fetch: (
      packageId: string,
      resolution: AtomicResolution,
      opts: FetchOptions
    ) => Promise<FetchResult>
    fetchingLocker: Map<string, FetchLock>
    getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
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
    const { filesIndexFile, target, resolution } = getFilesIndexFilePath(ctx, opts)

    doFetchToStore(filesIndexFile, fetching, target, resolution) // eslint-disable-line

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
    fetching: DeferredPromise<PkgRequestFetchResult>,
    target: string,
    resolution: AtomicResolution
  ) {
    try {
      const isLocalTarballDep = opts.pkg.id.startsWith('file:')
      const isLocalPkg = resolution.type === 'directory'

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
              opts.pkg?.name != null &&
              pkgFilesIndex.name.toLowerCase() !== opts.pkg.name.toLowerCase()
            ) ||
            (
              pkgFilesIndex.version != null &&
              opts.pkg?.version != null &&
              // We used to not normalize the package versions before writing them to the lockfile and store.
              // So it may happen that the version will be in different formats.
              // For instance, v1.0.0 and 1.0.0
              // Hence, we need to use semver.eq() to compare them.
              !equalOrSemverEqual(pkgFilesIndex.version, opts.pkg.version)
            )
          ) {
            const msg = `Package name mismatch found while reading ${JSON.stringify(opts.pkg.resolution)} from the store.`
            const hint = `This means that either the lockfile is broken or the package metadata (name and version) inside the package's package.json file doesn't match the metadata in the registry. \
Expected package: ${opts.pkg.name}@${opts.pkg.version}. \
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
        resolution,
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
  resolution: AtomicResolution,
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
